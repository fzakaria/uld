//! Core Linker logic.
//!
//! This module contains the `Linker` struct which orchestrates the entire linking process:
//! 1. Input Loading: Reads object files (and Archives).
//! 2. Symbol Resolution: Builds a global symbol table.
//! 3. Layout: Maps input sections to output segments and assigns virtual addresses.
//! 4. Relocation: Applies patches to code/data based on symbol addresses.
//! 5. Output: Writes the final ELF executable.

use anyhow::{Context, Result, anyhow};
use memmap2::Mmap;
use object::read::{Object, ObjectSection, RelocationTarget, SectionIndex};
use object::{ObjectSymbol, SectionKind, RelocationKind, SymbolKind};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::arch::Architecture;
use crate::layout::{Section, Segment};
use crate::utils::align_up;
use crate::writer;

const PAGE_SIZE: u64 = 0x1000;

// ELF x86_64 relocation types not mapped by object crate
const R_X86_64_GOTPCRELX: u32 = 41;
const R_X86_64_REX_GOTPCRELX: u32 = 42;

/// Check if a relocation requires a GOT entry (handles GOTPCRELX variants).
fn is_got_relocation(reloc: &object::read::Relocation) -> bool {
    match reloc.kind() {
        RelocationKind::Got | RelocationKind::GotRelative => return true,
        _ => {}
    }
    // Check raw ELF flags for GOTPCRELX variants
    if let object::RelocationFlags::Elf { r_type } = reloc.flags() {
        if r_type == R_X86_64_GOTPCRELX || r_type == R_X86_64_REX_GOTPCRELX {
            return true;
        }
    }
    false
}
const BASE_ADDR: u64 = 0x400000;

/// Representation of a symbol defined in one of the input object files.
pub struct DefinedSymbol {
    pub input_file_index: usize,
    pub section_index: SectionIndex,
    pub value: u64,
    pub is_weak: bool,
    pub is_absolute: bool,
}

pub struct Linker<'a, A: Architecture> {
    arch: A,
    input_objects: Vec<object::File<'a>>,
    input_paths: Vec<String>,
    symbol_table: HashMap<String, DefinedSymbol>,
    segments: Vec<Segment>,
    section_map: HashMap<(usize, SectionIndex), (usize, u64)>,
    got_map: HashMap<String, u64>,
    /// Symbols that are allowed to remain undefined (Weak, Hidden, or Internal Markers) 
    /// in a static binary, resolving to address 0.
    allowed_undefined: HashSet<String>,
}

impl<'a, A: Architecture> Linker<'a, A> {
    pub fn new(arch: A) -> Self {
        Self {
            arch,
            input_objects: Vec::new(),
            input_paths: Vec::new(),
            symbol_table: HashMap::new(),
            segments: Vec::new(),
            section_map: HashMap::new(),
            got_map: HashMap::new(),
            allowed_undefined: HashSet::new(),
        }
    }

    pub fn add_file(&mut self, path: PathBuf, mmap: &'a Mmap) -> Result<()> {
        let magic = &mmap[..8.min(mmap.len())];
        if magic.starts_with(b"!<arch>\n") {
             let archive = object::read::archive::ArchiveFile::parse(&**mmap)?;
             for member in archive.members() {
                 let member = member?;
                 let name = String::from_utf8_lossy(member.name()).to_string();
                 let data = member.data(&**mmap)?;
                 let obj = if data.as_ptr().align_offset(8) != 0 {
                     let vec = data.to_vec();
                     let leaked: &'a [u8] = Box::leak(vec.into_boxed_slice());
                     object::File::parse(leaked).context("failed to parse archive member")?
                 } else {
                     object::File::parse(data).context("failed to parse archive member")?
                 };
                 let member_path = format!("{}({})", path.display(), name);
                 self.process_object(member_path, obj)?;
             }
        } else {
             let obj = object::File::parse(&**mmap).context("failed to parse object file")?;
             self.process_object(path.display().to_string(), obj)?;
        }
        Ok(())
    }

    /// Check if a symbol is a compiler-internal marker or optional feature
    /// that should resolve to address 0 if not defined.
    fn is_stub_symbol(&self, name: &str, sym: Option<&object::Symbol>) -> bool {
        if let Some(s) = sym {
            if s.is_weak() { return true; }
            if let object::SymbolFlags::Elf { st_other, .. } = s.flags() {
                if (st_other & 0x03) == 2 { return true; }
            }
            if s.kind() == SymbolKind::Tls && s.is_undefined() { return true; }
        }

        name.starts_with("__TMC_") 
            || name.starts_with("__bid_") 
            || name.starts_with("__gcc_")
            || name.starts_with("__morestack")
            || name == "__dso_handle"
            || name == "_DYNAMIC"
            || name == "_dl_find_object"
    }

    fn process_object(&mut self, path: String, obj: object::File<'a>) -> Result<()> {
        let file_index = self.input_objects.len();
        for sym in obj.symbols() {
            let name = sym.name()?;
            if sym.is_undefined() {
                if self.is_stub_symbol(name, Some(&sym)) {
                    self.allowed_undefined.insert(name.to_string());
                }
                continue;
            }

            if sym.is_local() { continue; }
            let is_weak = sym.is_weak();
            if let Some(existing) = self.symbol_table.get(name) {
                if !is_weak && existing.is_weak { /* overwrite */ } else { continue; }
            }

            if let Some(section_index) = sym.section_index() {
                 self.symbol_table.insert(name.to_string(), DefinedSymbol {
                    input_file_index: file_index,
                    section_index,
                    value: sym.address(),
                    is_weak,
                    is_absolute: false,
                });
            } else {
                 self.symbol_table.insert(name.to_string(), DefinedSymbol {
                    input_file_index: file_index,
                    section_index: SectionIndex(0), 
                    value: sym.address(),
                    is_weak,
                    is_absolute: true,
                });
            }
        }
        self.input_objects.push(obj);
        self.input_paths.push(path);
        Ok(())
    }

    pub fn layout(&mut self) -> Result<()> {
        self.segments.push(Segment::new(".text", SectionKind::Text));
        self.segments.push(Segment::new(".rodata", SectionKind::ReadOnlyData));
        self.segments.push(Segment::new(".data", SectionKind::Data));
        self.segments.push(Segment::new(".bss", SectionKind::UninitializedData));
        self.segments.push(Segment::new(".got", SectionKind::Data));
        self.segments.push(Segment::new(".tdata", SectionKind::Tls)); // Map TLS symbols

        for (file_index, obj) in self.input_objects.iter().enumerate() {
            for section in obj.sections() {
                let size = section.size();
                if size == 0 { continue; }
                let kind = section.kind();
                let segment_idx = match kind {
                    SectionKind::Text => 0,
                    SectionKind::ReadOnlyData | SectionKind::ReadOnlyString => 1,
                    SectionKind::Data => 2,
                    SectionKind::UninitializedData => 3,
                    SectionKind::Tls => 5, // Keep TLS sections
                    // Handle init/fini arrays - ELF types 14 (SHT_INIT_ARRAY) and 15 (SHT_FINI_ARRAY)
                    SectionKind::Elf(14) | SectionKind::Elf(15) => 2, // Put in .data segment
                    _ => {
                        tracing::debug!("Skipping section {} (kind: {:?}, size: {})",
                            section.name().unwrap_or("?"), kind, size);
                        continue;
                    }
                };
                let segment = &mut self.segments[segment_idx];
                let start_offset = align_up(segment.size, section.align().max(1));
                segment.size = start_offset + size;
                if segment.kind != SectionKind::UninitializedData {
                    segment.data.resize(start_offset as usize, 0);
                    segment.data.extend_from_slice(section.data()?);
                }
                segment.sections.push(Section { file_index, section_index: section.index(), offset: start_offset });
                self.section_map.insert((file_index, section.index()), (segment_idx, start_offset));
            }
        }
        
        let mut got_offset = 0;
        for obj in &self.input_objects {
            for section in obj.sections() {
                for (_, reloc) in section.relocations() {
                    let mut needs_got = is_got_relocation(&reloc);

                    // Also check for TLS symbols
                    if !needs_got {
                        if let RelocationTarget::Symbol(idx) = reloc.target() {
                            if let Ok(sym) = obj.symbol_by_index(idx) {
                                if sym.kind() == SymbolKind::Tls {
                                    needs_got = true;
                                }
                            }
                        }
                    }

                    if needs_got {
                        if let RelocationTarget::Symbol(idx) = reloc.target() {
                            let sym = obj.symbol_by_index(idx)?;
                            let name = sym.name()?;
                            if !self.got_map.contains_key(name) {
                                self.got_map.insert(name.to_string(), got_offset);
                                got_offset += 8;
                            }
                        }
                    }
                }
            }
        }
        
        if let Some(got_seg) = self.segments.iter_mut().find(|s| s.name == ".got") {
            got_seg.size = got_offset;
            got_seg.data.resize(got_offset as usize, 0);
        }

        let mut current_va = BASE_ADDR + PAGE_SIZE;
        let mut current_off = PAGE_SIZE;
        for segment in &mut self.segments {
            if segment.size == 0 { continue; }
            current_va = align_up(current_va, PAGE_SIZE);
            current_off = align_up(current_off, PAGE_SIZE);
            segment.virtual_address = current_va;
            segment.file_offset = current_off;
            current_va += segment.size;
            if segment.kind != SectionKind::UninitializedData {
                current_off += segment.size;
            }
        }
        Ok(())
    }

    fn resolve_symbol_va(&self, file_index: usize, sym: &object::Symbol) -> Result<u64> {
        if sym.kind() == SymbolKind::Section {
            let sec_idx = sym.section_index().context("section symbol without index")?;
            return Ok(self.get_section_addr(file_index, sec_idx).unwrap_or(0));
        }

        if sym.is_local() {
            if let Some(sec_idx) = sym.section_index() {
                let base = self.get_section_addr(file_index, sec_idx).unwrap_or(0);
                return Ok(base + sym.address());
            }
            return Ok(sym.address());
        }

        let name = sym.name()?;
        if let Some(addr) = self.get_symbol_addr(name) {
            return Ok(addr);
        }

        Err(anyhow!(
            "symbol missing: name={}, file={}",
            name, self.input_paths[file_index]
        ))
    }

    fn verify_unresolved(&self) -> Result<()> {
        for (file_idx, obj) in self.input_objects.iter().enumerate() {
            for section in obj.sections() {
                for (_, reloc) in section.relocations() {
                    if let RelocationTarget::Symbol(idx) = reloc.target() {
                        let sym = obj.symbol_by_index(idx)?;
                        let name = sym.name()?;
                        if sym.is_undefined() && !self.is_stub_symbol(name, Some(&sym)) {
                            self.resolve_symbol_va(file_idx, &sym)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn fill_got(&mut self) -> Result<()> {
        let mut updates = Vec::new();
        for (name, offset) in &self.got_map {
            let addr = self.get_symbol_addr(name).unwrap_or(0);
            updates.push((*offset, addr));
        }
        if let Some(got_seg) = self.segments.iter_mut().find(|s| s.name == ".got") {
            for (offset, addr) in updates {
                let bytes = addr.to_le_bytes();
                got_seg.data[offset as usize..offset as usize + 8].copy_from_slice(&bytes);
            }
        }
        Ok(())
    }

    pub fn relocate(&mut self) -> Result<()> {
        self.verify_unresolved()?;
        self.fill_got()?;
        let got_va = self.segments.iter().find(|s| s.name == ".got").map(|s| s.virtual_address).unwrap_or(0);

        for seg_idx in 0..self.segments.len() {
            let mut patches = Vec::new();
            {
                let segment = &self.segments[seg_idx];
                for input_section in &segment.sections {
                    let obj = &self.input_objects[input_section.file_index];
                    let section = obj.section_by_index(input_section.section_index)?;
                    let section_va = segment.virtual_address + input_section.offset;

                    for (offset, reloc) in section.relocations() {
                        let target_va = match reloc.target() {
                            RelocationTarget::Symbol(idx) => {
                                let sym = obj.symbol_by_index(idx)?;
                                let name = sym.name()?;

                                // Check for GOT usage (handles GOTPCRELX variants) or TLS
                                let use_got = is_got_relocation(&reloc) || sym.kind() == SymbolKind::Tls;

                                if use_got {
                                    let g_offset = self.got_map.get(name).cloned().context("GOT entry missing")?;
                                    got_va + g_offset
                                } else {
                                    self.resolve_symbol_va(input_section.file_index, &sym)?
                                }
                            }
                            RelocationTarget::Section(sec_idx) => {
                                self.get_section_addr(input_section.file_index, sec_idx).unwrap_or(0)
                            }
                            _ => continue,
                        };

                        patches.push((input_section.offset + offset, reloc, section_va + offset, target_va));
                    }
                }
            }
            
            let segment_data = &mut self.segments[seg_idx].data;
            for (off, reloc, p, s) in patches {
                let addend = reloc.addend(); 
                self.arch.apply_relocation(off, &reloc, p, s, addend, segment_data)?;
            }
        }
        Ok(())
    }

    fn get_symbol_addr(&self, name: &str) -> Option<u64> {
        if name == "_GLOBAL_OFFSET_TABLE_" { 
            return self.segments.iter().find(|s| s.name == ".got").map(|s| s.virtual_address);
        }
        
        // 1. ALWAYS check the symbol table first.
        // If a symbol is defined, we must use its real address.
        if let Some(sym) = self.symbol_table.get(name) {
            if sym.is_absolute { return Some(sym.value); }
            if let Some((seg_idx, off)) = self.section_map.get(&(sym.input_file_index, sym.section_index)) {
                return Some(self.segments[*seg_idx].virtual_address + off + sym.value);
            }
        }

        // 2. Only if the symbol is NOT in the table, check if it's an allowed undefined stub.
        if self.allowed_undefined.contains(name) || self.is_stub_symbol(name, None) {
            return Some(0);
        }

        None
    }

    fn get_section_addr(&self, file_index: usize, section_index: SectionIndex) -> Option<u64> {
        let (seg_idx, offset) = self.section_map.get(&(file_index, section_index))?;
        Some(self.segments[*seg_idx].virtual_address + offset)
    }

    pub fn write(&self, output_path: &PathBuf) -> Result<()> {
        let entry_point = self.get_symbol_addr("_start").unwrap_or(0);
        writer::write_elf(output_path, &self.segments, entry_point)
    }
}