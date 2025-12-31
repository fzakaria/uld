//! Core Linker logic.
//!
//! This module contains the `Linker` struct which orchestrates the entire linking process:
//! 1. Input Loading: Reads object files (and Archives).
//! 2. Symbol Resolution: Builds a global symbol table.
//! 3. Layout: Maps input sections to output segments and assigns virtual addresses.
//! 4. Relocation: Applies patches to code/data based on symbol addresses.
//! 5. Output: Writes the final ELF executable.

use anyhow::{Context, Result};
use memmap2::Mmap;
use object::read::{Object, ObjectSection, RelocationTarget, SectionIndex};
use object::{ObjectSymbol, SectionKind, Endianness};
use object::endian::{U16, U32, U64};
use object::pod::bytes_of;
use std::collections::HashMap;
use std::path::PathBuf;
use std::os::unix::fs::PermissionsExt;

use crate::arch::Architecture;
use crate::layout::{Section, Segment};
use crate::symbol::DefinedSymbol;
use crate::utils::align_up;

// Constants for x86_64 Linux memory layout
const PAGE_SIZE: u64 = 0x1000;
const BASE_ADDR: u64 = 0x400000; 

// Helpers for Endianness types
fn u16(v: u16) -> U16<Endianness> { U16::new(Endianness::Little, v) }
fn u32(v: u32) -> U32<Endianness> { U32::new(Endianness::Little, v) }
fn u64(v: u64) -> U64<Endianness> { U64::new(Endianness::Little, v) }

/// The main Linker struct.
pub struct Linker<'a, A: Architecture> {
    /// The target architecture backend.
    arch: A,
    /// List of parsed objects.
    input_objects: Vec<object::File<'a>>,
    /// Original path for each object (for error reporting).
    input_paths: Vec<String>,
    /// Global symbol table: Name -> Definition.
    symbol_table: HashMap<String, DefinedSymbol>,
    /// List of output segments (.text, .data, etc.).
    segments: Vec<Segment>,
    /// Map (Object Index, SectionIndex) -> (SegmentIndex, Offset within Segment).
    section_map: HashMap<(usize, SectionIndex), (usize, u64)>,
    /// Map Symbol Name -> Offset in .got segment.
    got_map: HashMap<String, u64>,
}

impl<'a, A: Architecture> Linker<'a, A> {
    /// Creates a new Linker instance for a specific architecture.
    pub fn new(arch: A) -> Self {
        Self {
            arch,
            input_objects: Vec::new(),
            input_paths: Vec::new(),
            symbol_table: HashMap::new(),
            segments: Vec::new(),
            section_map: HashMap::new(),
            got_map: HashMap::new(),
        }
    }

    /// Adds a file (Object or Archive) to the linker.
    pub fn add_file(&mut self, path: PathBuf, mmap: &'a Mmap) -> Result<()> {
        let magic = &mmap[..8.min(mmap.len())];
        if magic.starts_with(b"!<arch>\n") {
             let archive = object::read::archive::ArchiveFile::parse(&**mmap)?;
             for member in archive.members() {
                 let member = member?;
                 let name = String::from_utf8_lossy(member.name()).to_string();
                 let data = member.data(&**mmap)?;
                 
                 // Handle alignment for ELF parsing
                 let obj = if data.as_ptr().align_offset(8) != 0 {
                     // If misaligned (common in archives), we must copy to aligned memory.
                     let vec = data.to_vec();
                     let leaked: &'a [u8] = Box::leak(vec.into_boxed_slice());
                     object::File::parse(leaked).context("failed to parse archive member (aligned copy)")?
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

    fn process_object(&mut self, path: String, obj: object::File<'a>) -> Result<()> {
        let file_index = self.input_objects.len();
        
        for sym in obj.symbols() {
            if sym.is_undefined() || sym.is_local() {
                continue;
            }

            let name = sym.name().context("failed to parse symbol name")?;
            let is_weak = sym.is_weak();
            
            if let Some(existing) = self.symbol_table.get(name) {
                if existing.is_weak && !is_weak {
                    // Upgrade weak to strong
                } else if !existing.is_weak && is_weak {
                    // Ignore weak if strong exists
                    continue; 
                } else if !existing.is_weak && !is_weak {
                     // anyhow::bail!("duplicate symbol: {} in {}", name, path);
                }
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
                 // No section index means it's an absolute symbol (or something special)
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

    /// Verifies that all undefined symbols in input files are resolved by the global symbol table.
    pub fn verify_unresolved(&self) -> Result<()> {
        for (i, obj) in self.input_objects.iter().enumerate() {
            for sym in obj.symbols() {
                if sym.is_undefined() {
                    let name = sym.name().unwrap_or("<unparsable>");
                    // Weak undefined symbols are allowed (value 0).
                    if sym.is_weak() {
                        continue;
                    }
                    // Handle runtime symbols provided by the linker or dummy stubs
                    if name == "_GLOBAL_OFFSET_TABLE_" || name == "_dl_find_object" || name == "_DYNAMIC" || name == "__TMC_END__" || 
                       name.starts_with("__bid_") || name.starts_with("__morestack") || name.starts_with("_ITM_") || name.starts_with("__real_") ||
                       name.contains("_array_start") || name.contains("_array_end") {
                        continue;
                    }
                    if !self.symbol_table.contains_key(name) {
                        anyhow::bail!("undefined reference: {} in {}", name, self.input_paths[i]);
                    }
                }
            }
        }
        Ok(())
    }

    /// Lays out the output segments.
    pub fn layout(&mut self) -> Result<()> {
        self.segments.push(Segment::new(".text", SectionKind::Text));
        self.segments.push(Segment::new(".rodata", SectionKind::ReadOnlyData));
        self.segments.push(Segment::new(".data", SectionKind::Data));
        self.segments.push(Segment::new(".bss", SectionKind::UninitializedData));
        self.segments.push(Segment::new(".got", SectionKind::Data));

        for (file_index, obj) in self.input_objects.iter().enumerate() {
            for section in obj.sections() {
                let size = section.size();
                if size == 0 { continue; }

                let align = section.align();
                let kind = section.kind();
                
                let segment_idx = match kind {
                    SectionKind::Text => 0,
                    SectionKind::ReadOnlyData | SectionKind::ReadOnlyString => 1,
                    SectionKind::Data => 2,
                    SectionKind::UninitializedData => 3,
                    _ => {
                        let name = section.name().unwrap_or("");
                        if name == ".init" || name == ".fini" {
                            0
                        } else if name == ".init_array" || name == ".fini_array" || name == ".preinit_array" {
                            2
                        } else if name == ".eh_frame" || name == ".eh_frame_hdr" || name == ".gcc_except_table" || name.starts_with(".note") {
                            1
                        } else if kind == SectionKind::Other || kind == SectionKind::Note {
                            1
                        } else {
                            tracing::trace!("Skipping section {} of kind {:?}", name, kind);
                            continue;
                        }
                    }
                };

                let segment = &mut self.segments[segment_idx];
                
                let start_offset = align_up(segment.size, align);
                let padding = start_offset - segment.size;
                segment.size += padding + size;
                
                if segment.kind != SectionKind::UninitializedData {
                    segment.data.resize(start_offset as usize, 0);
                    let data = section.data()?;
                    segment.data.extend_from_slice(data);
                }

                segment.sections.push(Section {
                    file_index,
                    section_index: section.index(),
                    offset: start_offset,
                });
                
                self.section_map.insert((file_index, section.index()), (segment_idx, start_offset));
            }
        }
        
        // Scan for GOT entries
        let mut current_got_offset = 0;
        
        // Reserve 16 bytes for a dummy _DYNAMIC at the start of GOT
        if !self.got_map.contains_key("_DYNAMIC") {
            self.got_map.insert("_DYNAMIC".to_string(), current_got_offset);
            current_got_offset += 16;
        }

        for obj in &self.input_objects {
            for section in obj.sections() {
                for (_, reloc) in section.relocations() {
                    match reloc.kind() {
                        object::RelocationKind::Got | object::RelocationKind::GotRelative => {
                            if let RelocationTarget::Symbol(idx) = reloc.target() {
                                if let Ok(sym) = obj.symbol_by_index(idx) {
                                    if let Ok(name) = sym.name() {
                                        if !self.got_map.contains_key(name) {
                                            self.got_map.insert(name.to_string(), current_got_offset);
                                            current_got_offset += 8;
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Resize .got segment
        let got_seg = &mut self.segments[4];
        got_seg.size = current_got_offset;
        got_seg.data.resize(current_got_offset as usize, 0);

        let mut current_va = BASE_ADDR + PAGE_SIZE; 
        let mut current_file_offset = PAGE_SIZE; 
        
        for segment in &mut self.segments {
            if segment.size == 0 { continue; }
            
            current_va = align_up(current_va, PAGE_SIZE);
            current_file_offset = align_up(current_file_offset, PAGE_SIZE);

            segment.virtual_address = current_va;
            segment.file_offset = current_file_offset;
            
            if segment.kind != SectionKind::UninitializedData {
                current_file_offset += segment.size;
            }
            
            current_va += segment.size;
        }

        Ok(())
    }

    fn fill_got(&mut self) -> Result<()> {
        // Collect updates first to satisfy borrow checker
        let mut updates = Vec::new();
        for (name, offset) in &self.got_map {
            let addr = self.get_symbol_addr(name).unwrap_or(0);
            updates.push((*offset, addr));
        }

        let got_data = &mut self.segments[4].data;
        for (offset, addr) in updates {
            let bytes = addr.to_le_bytes();
            let off = offset as usize;
            if off + 8 <= got_data.len() {
                got_data[off..off+8].copy_from_slice(&bytes);
            }
        }
        Ok(())
    }

    fn get_symbol_addr(&self, name: &str) -> Option<u64> {
        let got_va = self.segments[4].virtual_address;
        if name == "_GLOBAL_OFFSET_TABLE_" { return Some(got_va); }
        
        if let Some(got_offset) = self.got_map.get(name) {
            if name == "_DYNAMIC" {
                return Some(got_va + got_offset);
            }
        }
        
        // Dummy values for missing runtime symbols to allow linking
        if name == "_dl_find_object" || name == "__TMC_END__" || 
           name.starts_with("__bid_") || name.starts_with("__morestack") || name.starts_with("_ITM_") || name.starts_with("__real_") ||
           name.contains("_array_start") || name.contains("_array_end") {
            return Some(0);
        }

        let sym = self.symbol_table.get(name)?;
        if sym.is_absolute {
            return Some(sym.value);
        }
        let (seg_idx, offset) = self.section_map.get(&(sym.input_file_index, sym.section_index))?;
        Some(self.segments[*seg_idx].virtual_address + offset + sym.value)
    }

    fn get_section_addr(&self, file_index: usize, section_index: SectionIndex) -> Option<u64> {
        let (seg_idx, offset) = self.section_map.get(&(file_index, section_index))?;
        Some(self.segments[*seg_idx].virtual_address + offset)
    }

    /// Applies relocations to the output segments.
    pub fn relocate(&mut self) -> Result<()> {
        self.fill_got()?;

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
                                let res = if reloc.kind() == object::RelocationKind::GotRelative {
                                    let name = sym.name()?;
                                    let got_offset = self.got_map.get(name).cloned().context("GOT entry missing")?;
                                    let got_va = self.segments[4].virtual_address;
                                    got_va + got_offset
                                } else if sym.is_undefined() {
                                    let name = sym.name()?;
                                    self.get_symbol_addr(name).context(format!("undefined symbol: {}", name))?
                                } else if sym.kind() == object::SymbolKind::Section {
                                    let sec_idx = sym.section_index().unwrap();
                                    self.get_section_addr(input_section.file_index, sec_idx).unwrap_or(0) + sym.address()
                                } else if sym.is_local() {
                                     if let Some(sec_idx) = sym.section_index() {
                                         self.get_section_addr(input_section.file_index, sec_idx).unwrap_or(0) + sym.address()
                                     } else {
                                         sym.address()
                                     }
                                } else {
                                     let name = sym.name()?;
                                     self.get_symbol_addr(name).context(format!("undefined symbol: {}", name))?
                                };
                                tracing::trace!("Relocation at {:x} target symbol {} VA {:x}", section_va + offset, sym.name().unwrap_or("?"), res);
                                res
                            }
                            RelocationTarget::Section(sec_idx) => {
                                let res = self.get_section_addr(input_section.file_index, sec_idx).unwrap_or(0);
                                tracing::trace!("Relocation at {:x} target section {:?} VA {:x}", section_va + offset, sec_idx, res);
                                res
                            }
                            _ => continue,
                        };

                        patches.push((input_section.offset + offset, reloc, section_va + offset, target_va));
                    }
                }
            }
            
            let segment_data = &mut self.segments[seg_idx].data;
            for (offset, reloc, p, s) in patches {
                let addend = reloc.addend(); 
                self.arch.apply_relocation(offset, &reloc, p, s, addend, segment_data)?;
            }
        }
        Ok(())
    }
    
    /// Writes the final ELF executable to the output path.
    pub fn write(&self, output_path: &PathBuf) -> Result<()> {
        let mut buffer = Vec::new();
        let num_sections = self.segments.len() as u32 + 2; 

        // Write File Header
        let file_header = object::elf::FileHeader64::<Endianness> {
            e_ident: object::elf::Ident {
                magic: object::elf::ELFMAG,
                class: object::elf::ELFCLASS64,
                data: object::elf::ELFDATA2LSB,
                version: object::elf::EV_CURRENT,
                os_abi: object::elf::ELFOSABI_SYSV,
                abi_version: 0,
                padding: [0; 7],
            },
            e_type: u16(object::elf::ET_EXEC),
            e_machine: u16(object::elf::EM_X86_64),
            e_version: u32(object::elf::EV_CURRENT as u32),
            e_entry: u64(self.get_symbol_addr("_start").unwrap_or(0)),
            e_phoff: u64(64),
            e_shoff: u64(0), 
            e_flags: u32(0),
            e_ehsize: u16(64),
            e_phentsize: u16(56),
            e_phnum: u16(1),
            e_shentsize: u16(64),
            e_shnum: u16(num_sections as u16), 
            e_shstrndx: u16(num_sections as u16 - 1),
        };
        buffer.extend_from_slice(bytes_of(&file_header));

        let last_segment = self.segments.iter() 
            .filter(|s| s.kind != SectionKind::UninitializedData && s.size > 0)
            .last();

        let file_size = if let Some(seg) = last_segment {
                seg.file_offset + seg.size
        } else {
                PAGE_SIZE
        };
        
        let mem_size = self.segments.iter()
            .map(|s| if s.virtual_address > 0 { s.virtual_address + s.size } else { BASE_ADDR })
            .max().unwrap_or(BASE_ADDR) - BASE_ADDR;

        let prog_header = object::elf::ProgramHeader64::<Endianness> {
            p_type: u32(object::elf::PT_LOAD),
            p_flags: u32(object::elf::PF_R | object::elf::PF_W | object::elf::PF_X),
            p_offset: u64(0),
            p_vaddr: u64(BASE_ADDR),
            p_paddr: u64(BASE_ADDR),
            p_filesz: u64(file_size),
            p_memsz: u64(mem_size),
            p_align: u64(PAGE_SIZE),
        };
        buffer.extend_from_slice(bytes_of(&prog_header));

        // Pad to PAGE_SIZE
        if (buffer.len() as u64) < PAGE_SIZE {
            buffer.resize(PAGE_SIZE as usize, 0);
        }

        for segment in &self.segments {
            if segment.kind == SectionKind::UninitializedData { continue; }
            let current = buffer.len() as u64; 
            if segment.file_offset > current {
                buffer.resize(segment.file_offset as usize, 0);
            }
            buffer.extend_from_slice(&segment.data);
        }
        
        // Build shstrtab
        let mut shstrtab = Vec::new();
        shstrtab.push(0); 
        let mut section_name_offsets = Vec::new();
        section_name_offsets.push(0);
        
        for segment in &self.segments {
            let off = shstrtab.len();
            section_name_offsets.push(off);
            shstrtab.extend_from_slice(segment.name.as_bytes());
            shstrtab.push(0);
        }
        
        let shstrtab_offset = shstrtab.len();
        section_name_offsets.push(shstrtab_offset);
        shstrtab.extend_from_slice(b".shstrtab\0");
        
        let shoff = buffer.len(); 
        
        // 0: Null
        let null_sec = object::elf::SectionHeader64::<Endianness> {
            sh_name: u32(0),
            sh_type: u32(object::elf::SHT_NULL),
            sh_flags: u64(0),
            sh_addr: u64(0),
            sh_offset: u64(0),
            sh_size: u64(0),
            sh_link: u32(0),
            sh_info: u32(0),
            sh_addralign: u64(0),
            sh_entsize: u64(0),
        };
        buffer.extend_from_slice(bytes_of(&null_sec));
        
        for (i, segment) in self.segments.iter().enumerate() {
            let sec_header = object::elf::SectionHeader64::<Endianness> {
                sh_name: u32(section_name_offsets[i+1] as u32),
                sh_type: u32(if segment.kind == SectionKind::UninitializedData { object::elf::SHT_NOBITS } else { object::elf::SHT_PROGBITS }),
                sh_flags: u64(match segment.kind {
                    SectionKind::Text => object::elf::SHF_ALLOC | object::elf::SHF_EXECINSTR,
                    SectionKind::Data => object::elf::SHF_ALLOC | object::elf::SHF_WRITE,
                    SectionKind::UninitializedData => object::elf::SHF_ALLOC | object::elf::SHF_WRITE,
                    _ => object::elf::SHF_ALLOC,
                } as u64),
                sh_addr: u64(segment.virtual_address),
                sh_offset: u64(segment.file_offset),
                sh_size: u64(segment.size),
                sh_link: u32(0),
                sh_info: u32(0),
                sh_addralign: u64(16),
                sh_entsize: u64(0),
            };
            buffer.extend_from_slice(bytes_of(&sec_header));
        }
        
        let shstrtab_header = object::elf::SectionHeader64::<Endianness> {
            sh_name: u32(section_name_offsets[section_name_offsets.len()-1] as u32),
            sh_type: u32(object::elf::SHT_STRTAB),
            sh_flags: u64(0),
            sh_addr: u64(0),
            sh_offset: u64((shoff + (num_sections as usize * 64)) as u64), 
            sh_size: u64(shstrtab.len() as u64),
            sh_link: u32(0),
            sh_info: u32(0),
            sh_addralign: u64(1),
            sh_entsize: u64(0),
        };
        buffer.extend_from_slice(bytes_of(&shstrtab_header));
        
        buffer.extend_from_slice(&shstrtab);
        
        let shoff_bytes = (shoff as u64).to_le_bytes();
        buffer[40..48].copy_from_slice(&shoff_bytes);
        
        std::fs::write(output_path, &buffer)?;
        
        let mut perms = std::fs::metadata(output_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(output_path, perms)?;

        Ok(())
    }
}
