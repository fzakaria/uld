//! Simple ELF Linker
//!
//! Combines object files into a static executable:
//! 1. Load object files and archives
//! 2. Build a global symbol table
//! 3. Layout sections into segments with virtual addresses
//! 4. Apply relocations (patch code/data with resolved addresses)
//! 5. Write the ELF executable

use anyhow::{Context, Result, anyhow};
use memmap2::Mmap;
use object::read::{Object, ObjectSection, RelocationTarget, SectionIndex};
use object::{ObjectSymbol, Relocation, RelocationKind, SectionKind, SymbolKind, SymbolVisibility};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::arch::Architecture;
use crate::layout::{Section, Segment};
use crate::utils::align_up;
use crate::writer;

const PAGE_SIZE: u64 = 0x1000;
const BASE_ADDR: u64 = 0x400000;

/// A symbol defined in an input object file.
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
    /// Symbols allowed to resolve to 0 (weak, hidden, linker internals)
    weak_undefined: HashSet<String>,
    /// Undefined symbols that drive selective archive linking
    undefined_symbols: HashSet<String>,
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
            weak_undefined: HashSet::new(),
            undefined_symbols: HashSet::new(),
        }
    }

    pub fn add_file(&mut self, path: PathBuf, mmap: &'a Mmap) -> Result<()> {
        if mmap.starts_with(b"!<arch>\n") {
            return self.add_archive(path, mmap);
        }
        let obj = object::File::parse(&**mmap)?;
        self.process_object(path.display().to_string(), obj)
    }

    /// Selective archive linking: only include members defining needed symbols.
    fn add_archive(&mut self, path: PathBuf, mmap: &'a Mmap) -> Result<()> {
        let archive = object::read::archive::ArchiveFile::parse(&**mmap)?;

        // Build symbol → member index
        let mut symbol_to_member: HashMap<String, (String, &'a [u8])> = HashMap::new();
        for member in archive.members() {
            let member = member?;
            let name = String::from_utf8_lossy(member.name()).to_string();
            let data = self.align_member_data(member.data(&**mmap)?);

            let Ok(obj) = object::File::parse(data) else { continue };
            for sym in obj.symbols() {
                let Ok(sym_name) = sym.name() else { continue };
                if sym.is_undefined() || sym.is_local() {
                    continue;
                }
                symbol_to_member.insert(sym_name.to_string(), (name.clone(), data));
            }
        }

        // Pull in members until no new symbols resolved
        let mut included = HashSet::new();
        loop {
            let mut added = false;
            for sym_name in self.undefined_symbols.clone() {
                let Some((member_name, data)) = symbol_to_member.get(&sym_name) else { continue };
                if included.contains(member_name) {
                    continue;
                }
                included.insert(member_name.clone());
                let obj = object::File::parse(*data)?;
                self.process_object(format!("{}({})", path.display(), member_name), obj)?;
                added = true;
            }
            if !added {
                break;
            }
        }
        Ok(())
    }

    /// Ensure 8-byte alignment for object parsing.
    fn align_member_data(&self, data: &'a [u8]) -> &'a [u8] {
        if data.as_ptr().align_offset(8) == 0 {
            return data;
        }
        Box::leak(data.to_vec().into_boxed_slice())
    }

    fn process_object(&mut self, path: String, obj: object::File<'a>) -> Result<()> {
        let file_index = self.input_objects.len();

        for sym in obj.symbols() {
            let name = sym.name()?;

            // Handle undefined symbols
            if sym.is_undefined() {
                self.record_undefined(name, &sym);
                continue;
            }

            // Skip local symbols
            if sym.is_local() {
                continue;
            }

            // Skip if already defined (unless existing is weak)
            if self.symbol_table.contains_key(name) {
                let dominated = self.symbol_table.get(name).is_some_and(|s| s.is_weak);
                if !dominated {
                    continue;
                }
            }

            self.undefined_symbols.remove(name);
            self.symbol_table.insert(name.to_string(), DefinedSymbol {
                input_file_index: file_index,
                section_index: sym.section_index().unwrap_or(SectionIndex(0)),
                value: sym.address(),
                is_weak: sym.is_weak(),
                is_absolute: sym.section_index().is_none(),
            });
        }

        self.input_objects.push(obj);
        self.input_paths.push(path);
        Ok(())
    }

    fn record_undefined(&mut self, name: &str, sym: &object::Symbol) {
        if self.is_optional_symbol(name, Some(sym)) {
            self.weak_undefined.insert(name.to_string());
            return;
        }
        if !self.symbol_table.contains_key(name) {
            self.undefined_symbols.insert(name.to_string());
        }
    }

    /// Check if symbol can remain undefined (resolves to 0).
    fn is_optional_symbol(&self, name: &str, sym: Option<&object::Symbol>) -> bool {
        // Weak or hidden symbols
        if let Some(s) = sym {
            if s.is_weak() || s.visibility() == SymbolVisibility::Hidden {
                return true;
            }
            if s.kind() == SymbolKind::Tls && s.is_undefined() {
                return true;
            }
        }
        // Known linker-internal symbols
        matches!(name, "_DYNAMIC" | "__dso_handle" | "_dl_find_object" | "__TMC_END__")
            || name.starts_with("__TMC_")
            || name.starts_with("__gcc_")
    }

    pub fn layout(&mut self) -> Result<()> {
        self.init_segments();
        self.place_sections()?;
        self.build_got()?;
        self.assign_addresses();
        Ok(())
    }

    fn init_segments(&mut self) {
        // Order matters: PROGBITS first, BSS last (no file content).
        // With single LOAD: vaddr = BASE_ADDR + file_offset.
        // BSS needs memory but no file space, so must be last.
        self.segments = vec![
            Segment::new(".text", SectionKind::Text),
            Segment::new(".init", SectionKind::Text),
            Segment::new(".fini", SectionKind::Text),
            Segment::new(".rodata", SectionKind::ReadOnlyData),
            Segment::new(".data", SectionKind::Data),
            Segment::new(".got", SectionKind::Data),
            Segment::new(".tdata", SectionKind::Tls),
            Segment::new(".bss", SectionKind::UninitializedData), // MUST BE LAST
        ];
    }

    fn place_sections(&mut self) -> Result<()> {
        for (file_index, obj) in self.input_objects.iter().enumerate() {
            for section in obj.sections() {
                if section.size() == 0 {
                    continue;
                }

                let Some(seg_idx) = self.section_to_segment(&section) else { continue };

                let segment = &mut self.segments[seg_idx];
                let offset = align_up(segment.size, section.align().max(1));
                segment.size = offset + section.size();

                if section.kind() != SectionKind::UninitializedData {
                    segment.data.resize(offset as usize, 0);
                    segment.data.extend_from_slice(section.data()?);
                }

                segment.sections.push(Section {
                    file_index,
                    section_index: section.index(),
                    offset,
                });
                self.section_map.insert((file_index, section.index()), (seg_idx, offset));
            }
        }
        Ok(())
    }

    fn section_to_segment(&self, section: &object::Section) -> Option<usize> {
        let name = section.name().unwrap_or("");
        match name {
            ".init" => Some(1),
            ".fini" => Some(2),
            _ => match section.kind() {
                SectionKind::Text => Some(0),
                SectionKind::ReadOnlyData | SectionKind::ReadOnlyString => Some(3),
                SectionKind::Data | SectionKind::Elf(14) | SectionKind::Elf(15) => Some(4),
                SectionKind::Tls => Some(6),
                SectionKind::UninitializedData => Some(7),
                _ => {
                    tracing::debug!("Skipping section {} ({:?})", name, section.kind());
                    None
                }
            },
        }
    }

    fn build_got(&mut self) -> Result<()> {
        let mut got_offset = 0u64;

        for obj in &self.input_objects {
            for section in obj.sections() {
                for (_, reloc) in section.relocations() {
                    if !self.needs_got(obj, &reloc) {
                        continue;
                    }
                    let RelocationTarget::Symbol(idx) = reloc.target() else { continue };
                    let name = obj.symbol_by_index(idx)?.name()?;

                    if self.got_map.contains_key(name) {
                        continue;
                    }
                    self.got_map.insert(name.to_string(), got_offset);
                    got_offset += 8;
                }
            }
        }

        let Some(got) = self.segments.iter_mut().find(|s| s.name == ".got") else { return Ok(()) };
        got.size = got_offset;
        got.data.resize(got_offset as usize, 0);
        Ok(())
    }

    fn needs_got(&self, obj: &object::File, reloc: &Relocation) -> bool {
        if matches!(reloc.kind(), RelocationKind::Got | RelocationKind::GotRelative) {
            return true;
        }
        // TLS symbols use GOT
        let RelocationTarget::Symbol(idx) = reloc.target() else { return false };
        obj.symbol_by_index(idx).is_ok_and(|s| s.kind() == SymbolKind::Tls)
    }

    fn assign_addresses(&mut self) {
        let mut vaddr = BASE_ADDR + PAGE_SIZE;
        let mut file_off = PAGE_SIZE;

        for segment in &mut self.segments {
            if segment.size == 0 {
                continue;
            }
            vaddr = align_up(vaddr, PAGE_SIZE);
            file_off = align_up(file_off, PAGE_SIZE);

            segment.virtual_address = vaddr;
            segment.file_offset = file_off;

            vaddr += segment.size;
            if segment.kind != SectionKind::UninitializedData {
                file_off += segment.size;
            }
        }
    }

    pub fn relocate(&mut self) -> Result<()> {
        self.fill_got();

        for seg_idx in 0..self.segments.len() {
            let patches = self.collect_patches(seg_idx)?;
            for (off, reloc, place, target) in patches {
                self.arch.apply_relocation(
                    off, &reloc, place, target, reloc.addend(),
                    &mut self.segments[seg_idx].data
                )?;
            }
        }
        Ok(())
    }

    fn fill_got(&mut self) {
        let entries: Vec<_> = self.got_map.iter()
            .map(|(name, &off)| (off, self.get_symbol_addr(name).unwrap_or(0)))
            .collect();

        let Some(got) = self.segments.iter_mut().find(|s| s.name == ".got") else { return };
        for (offset, addr) in entries {
            got.data[offset as usize..][..8].copy_from_slice(&addr.to_le_bytes());
        }
    }

    fn collect_patches(&self, seg_idx: usize) -> Result<Vec<(u64, Relocation, u64, u64)>> {
        let got_va = self.segments.iter()
            .find(|s| s.name == ".got")
            .map(|s| s.virtual_address)
            .unwrap_or(0);

        let mut patches = Vec::new();
        let sections = self.segments[seg_idx].sections.clone();

        for input_section in &sections {
            let obj = &self.input_objects[input_section.file_index];
            let section = obj.section_by_index(input_section.section_index)?;
            let section_va = self.segments[seg_idx].virtual_address + input_section.offset;

            for (offset, reloc) in section.relocations() {
                let Some(target_va) = self.resolve_reloc_target(obj, &reloc, input_section.file_index, got_va)? else {
                    continue;
                };
                patches.push((input_section.offset + offset, reloc, section_va + offset, target_va));
            }
        }
        Ok(patches)
    }

    fn resolve_reloc_target(
        &self,
        obj: &object::File,
        reloc: &Relocation,
        file_index: usize,
        got_va: u64,
    ) -> Result<Option<u64>> {
        match reloc.target() {
            RelocationTarget::Symbol(idx) => {
                let sym = obj.symbol_by_index(idx)?;
                let use_got = matches!(reloc.kind(), RelocationKind::Got | RelocationKind::GotRelative)
                    || sym.kind() == SymbolKind::Tls;

                if use_got {
                    let got_off = self.got_map.get(sym.name()?).context("GOT entry missing")?;
                    return Ok(Some(got_va + got_off));
                }
                Ok(Some(self.resolve_symbol(file_index, &sym)?))
            }
            RelocationTarget::Section(idx) => {
                Ok(Some(self.get_section_addr(file_index, idx).unwrap_or(0)))
            }
            _ => Ok(None),
        }
    }

    fn resolve_symbol(&self, file_index: usize, sym: &object::Symbol) -> Result<u64> {
        // Section symbols
        if sym.kind() == SymbolKind::Section {
            let idx = sym.section_index().context("section symbol without index")?;
            return Ok(self.get_section_addr(file_index, idx).unwrap_or(0));
        }

        // Local symbols: section base + offset
        if sym.is_local() {
            let base = sym.section_index()
                .and_then(|idx| self.get_section_addr(file_index, idx))
                .unwrap_or(0);
            return Ok(base + sym.address());
        }

        // Global symbols
        let name = sym.name()?;
        self.get_symbol_addr(name).ok_or_else(|| anyhow!("undefined symbol: {}", name))
    }

    fn get_symbol_addr(&self, name: &str) -> Option<u64> {
        // GOT base
        if name == "_GLOBAL_OFFSET_TABLE_" {
            return self.segments.iter().find(|s| s.name == ".got").map(|s| s.virtual_address);
        }

        // Symbol table lookup
        if let Some(sym) = self.symbol_table.get(name) {
            if sym.is_absolute {
                return Some(sym.value);
            }
            // Symbol may be in table but its section wasn't mapped (e.g., debug sections)
            if let Some(&(seg_idx, off)) = self.section_map.get(&(sym.input_file_index, sym.section_index)) {
                return Some(self.segments[seg_idx].virtual_address + off + sym.value);
            }
            // Fall through to optional check
        }

        // Weak/optional → 0
        if self.weak_undefined.contains(name) || self.is_optional_symbol(name, None) {
            return Some(0);
        }

        None
    }

    fn get_section_addr(&self, file_index: usize, section_index: SectionIndex) -> Option<u64> {
        let &(seg_idx, offset) = self.section_map.get(&(file_index, section_index))?;
        Some(self.segments[seg_idx].virtual_address + offset)
    }

    pub fn write(&self, output_path: &PathBuf) -> Result<()> {
        let entry = self.get_symbol_addr("_start").unwrap_or(0);
        writer::write_elf(output_path, &self.segments, entry)
    }
}
