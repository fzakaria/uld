//! ELF Static Linker
//!
//! Links object files and archives into a static executable:
//! 1. Parse objects, build symbol table
//! 2. Layout sections into segments
//! 3. Resolve symbol addresses
//! 4. Apply relocations
//! 5. Write ELF output

use anyhow::{Context, Result, anyhow};
use memmap2::Mmap;
use object::read::{Object, ObjectSection, RelocationTarget, SectionIndex};
use object::{ObjectSymbol, Relocation, RelocationKind, SectionKind, SymbolKind, SymbolVisibility};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::arch::Architecture;
use crate::layout::{Section, Segment};
use crate::symbol::{DefinedSymbol, is_optional_symbol};
use crate::utils::align_up;
use crate::writer;

const PAGE_SIZE: u64 = 0x1000;
const BASE_ADDR: u64 = 0x400000;

pub struct Linker<'a, A: Architecture> {
    arch: A,
    objects: Vec<object::File<'a>>,
    paths: Vec<String>,
    symbols: HashMap<String, DefinedSymbol>,
    segments: Vec<Segment>,
    /// Maps (file_idx, section_idx) → (segment_idx, offset)
    section_map: HashMap<(usize, SectionIndex), (usize, u64)>,
    /// GOT entries: symbol_name → offset in GOT
    got: HashMap<String, u64>,
    /// Symbols that can resolve to 0
    weak_undefined: HashSet<String>,
    /// Symbols needed for archive linking
    undefined: HashSet<String>,
}

impl<'a, A: Architecture> Linker<'a, A> {
    pub fn new(arch: A) -> Self {
        Self {
            arch,
            objects: Vec::new(),
            paths: Vec::new(),
            symbols: HashMap::new(),
            segments: Vec::new(),
            section_map: HashMap::new(),
            got: HashMap::new(),
            weak_undefined: HashSet::new(),
            undefined: HashSet::new(),
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Phase 1: Load files
    // ─────────────────────────────────────────────────────────────────────────

    pub fn add_file(&mut self, path: PathBuf, mmap: &'a Mmap) -> Result<()> {
        if mmap.starts_with(b"!<arch>\n") {
            return self.add_archive(path, mmap);
        }
        let obj = object::File::parse(&**mmap)?;
        self.add_object(path.display().to_string(), obj)
    }

    fn add_archive(&mut self, path: PathBuf, mmap: &'a Mmap) -> Result<()> {
        let archive = object::read::archive::ArchiveFile::parse(&**mmap)?;

        // Index: symbol → (member_name, data)
        let mut index: HashMap<String, (String, &'a [u8])> = HashMap::new();
        for member in archive.members() {
            let member = member?;
            let name = String::from_utf8_lossy(member.name()).to_string();
            let data = self.align_data(member.data(&**mmap)?);

            let Ok(obj) = object::File::parse(data) else { continue };
            for sym in obj.symbols() {
                let Ok(sym_name) = sym.name() else { continue };
                if sym.is_undefined() || sym.is_local() { continue }
                index.insert(sym_name.to_string(), (name.clone(), data));
            }
        }

        // Pull in members that define needed symbols
        let mut included = HashSet::new();
        loop {
            let mut added = false;
            for sym in self.undefined.clone() {
                let Some((member, data)) = index.get(&sym) else { continue };
                if included.contains(member) { continue }

                included.insert(member.clone());
                let obj = object::File::parse(*data)?;
                self.add_object(format!("{}({})", path.display(), member), obj)?;
                added = true;
            }
            if !added { break }
        }
        Ok(())
    }

    fn align_data(&self, data: &'a [u8]) -> &'a [u8] {
        if data.as_ptr().align_offset(8) == 0 { data }
        else { Box::leak(data.to_vec().into_boxed_slice()) }
    }

    fn add_object(&mut self, path: String, obj: object::File<'a>) -> Result<()> {
        let file_idx = self.objects.len();

        for sym in obj.symbols() {
            let name = sym.name()?;

            if sym.is_undefined() {
                self.record_undefined(name, &sym);
                continue;
            }
            if sym.is_local() { continue }

            // Strong overrides weak
            if self.symbols.contains_key(name) && !self.symbols[name].is_weak {
                continue;
            }

            self.undefined.remove(name);
            self.symbols.insert(name.to_string(), DefinedSymbol::new(
                file_idx,
                sym.section_index().unwrap_or(SectionIndex(0)),
                sym.address(),
                sym.is_weak(),
                sym.section_index().is_none(),
            ));
        }

        self.objects.push(obj);
        self.paths.push(path);
        Ok(())
    }

    fn record_undefined(&mut self, name: &str, sym: &object::Symbol) {
        let optional = sym.is_weak()
            || sym.visibility() == SymbolVisibility::Hidden
            || (sym.kind() == SymbolKind::Tls && sym.is_undefined())
            || is_optional_symbol(name);

        if optional {
            self.weak_undefined.insert(name.to_string());
        } else if !self.symbols.contains_key(name) {
            self.undefined.insert(name.to_string());
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Phase 2: Link (layout + resolve + relocate)
    // ─────────────────────────────────────────────────────────────────────────

    pub fn link(&mut self) -> Result<()> {
        self.layout()?;
        self.resolve_symbols();
        self.relocate()
    }

    fn layout(&mut self) -> Result<()> {
        // Segments in order. BSS must be last (no file content).
        self.segments = vec![
            Segment::new(".text", SectionKind::Text),
            Segment::new(".init", SectionKind::Text),
            Segment::new(".fini", SectionKind::Text),
            Segment::new(".rodata", SectionKind::ReadOnlyData),
            Segment::new(".data", SectionKind::Data),
            Segment::new(".got", SectionKind::Data),
            Segment::new(".tdata", SectionKind::Tls),
            Segment::new(".bss", SectionKind::UninitializedData),
        ];

        // Place sections
        for (file_idx, obj) in self.objects.iter().enumerate() {
            for section in obj.sections() {
                if section.size() == 0 { continue }

                let Some(seg_idx) = self.map_section(&section) else { continue };
                let segment = &mut self.segments[seg_idx];
                let offset = align_up(segment.size, section.align().max(1));

                segment.size = offset + section.size();
                if section.kind() != SectionKind::UninitializedData {
                    segment.data.resize(offset as usize, 0);
                    segment.data.extend_from_slice(section.data()?);
                }

                segment.sections.push(Section {
                    file_index: file_idx,
                    section_index: section.index(),
                    offset,
                });
                self.section_map.insert((file_idx, section.index()), (seg_idx, offset));
            }
        }

        // Build GOT
        self.build_got()?;

        // Assign addresses
        let mut vaddr = BASE_ADDR + PAGE_SIZE;
        let mut file_off = PAGE_SIZE;
        for seg in &mut self.segments {
            if seg.size == 0 { continue }
            vaddr = align_up(vaddr, PAGE_SIZE);
            file_off = align_up(file_off, PAGE_SIZE);
            seg.virtual_address = vaddr;
            seg.file_offset = file_off;
            vaddr += seg.size;
            if seg.kind != SectionKind::UninitializedData {
                file_off += seg.size;
            }
        }

        Ok(())
    }

    fn map_section(&self, section: &object::Section) -> Option<usize> {
        match section.name().unwrap_or("") {
            ".init" => Some(1),
            ".fini" => Some(2),
            _ => match section.kind() {
                SectionKind::Text => Some(0),
                SectionKind::ReadOnlyData | SectionKind::ReadOnlyString => Some(3),
                SectionKind::Data | SectionKind::Elf(14) | SectionKind::Elf(15) => Some(4),
                SectionKind::Tls => Some(6),
                SectionKind::UninitializedData => Some(7),
                _ => {
                    tracing::debug!("Skipping: {} ({:?})", section.name().unwrap_or("?"), section.kind());
                    None
                }
            }
        }
    }

    fn build_got(&mut self) -> Result<()> {
        let mut offset = 0u64;

        for obj in &self.objects {
            for section in obj.sections() {
                for (_, reloc) in section.relocations() {
                    if !self.needs_got(obj, &reloc) { continue }
                    let RelocationTarget::Symbol(idx) = reloc.target() else { continue };
                    let name = obj.symbol_by_index(idx)?.name()?;

                    if !self.got.contains_key(name) {
                        self.got.insert(name.to_string(), offset);
                        offset += 8;
                    }
                }
            }
        }

        if let Some(got) = self.segments.iter_mut().find(|s| s.name == ".got") {
            got.size = offset;
            got.data.resize(offset as usize, 0);
        }
        Ok(())
    }

    fn needs_got(&self, obj: &object::File, reloc: &Relocation) -> bool {
        matches!(reloc.kind(), RelocationKind::Got | RelocationKind::GotRelative)
            || matches!(reloc.target(), RelocationTarget::Symbol(idx)
                if obj.symbol_by_index(idx).is_ok_and(|s| s.kind() == SymbolKind::Tls))
    }

    /// Resolve all symbol addresses after layout.
    fn resolve_symbols(&mut self) {
        for sym in self.symbols.values_mut() {
            if sym.is_absolute {
                sym.resolved_address = Some(sym.offset);
                continue;
            }
            if let Some(&(seg_idx, off)) = self.section_map.get(&(sym.input_file_index, sym.section_index)) {
                sym.resolved_address = Some(self.segments[seg_idx].virtual_address + off + sym.offset);
            }
        }
    }

    fn relocate(&mut self) -> Result<()> {
        // Fill GOT
        let entries: Vec<_> = self.got.iter()
            .map(|(name, &off)| (off, self.symbol_address(name)))
            .collect();

        if let Some(got) = self.segments.iter_mut().find(|s| s.name == ".got") {
            for (off, addr) in entries {
                got.data[off as usize..][..8].copy_from_slice(&addr.to_le_bytes());
            }
        }

        // Apply relocations
        for seg_idx in 0..self.segments.len() {
            let patches = self.collect_patches(seg_idx)?;
            for (off, reloc, place, target) in patches {
                self.arch.apply_relocation(off, &reloc, place, target, reloc.addend(), &mut self.segments[seg_idx].data)?;
            }
        }
        Ok(())
    }

    fn collect_patches(&self, seg_idx: usize) -> Result<Vec<(u64, Relocation, u64, u64)>> {
        let got_va = self.got_address();
        let mut patches = Vec::new();

        for section in self.segments[seg_idx].sections.clone() {
            let obj = &self.objects[section.file_index];
            let sec = obj.section_by_index(section.section_index)?;
            let sec_va = self.segments[seg_idx].virtual_address + section.offset;

            for (off, reloc) in sec.relocations() {
                let Some(target) = self.resolve_reloc(obj, &reloc, section.file_index, got_va)? else { continue };
                patches.push((section.offset + off, reloc, sec_va + off, target));
            }
        }
        Ok(patches)
    }

    fn resolve_reloc(&self, obj: &object::File, reloc: &Relocation, file_idx: usize, got_va: u64) -> Result<Option<u64>> {
        match reloc.target() {
            RelocationTarget::Symbol(idx) => {
                let sym = obj.symbol_by_index(idx)?;
                let use_got = matches!(reloc.kind(), RelocationKind::Got | RelocationKind::GotRelative)
                    || sym.kind() == SymbolKind::Tls;

                if use_got {
                    let off = self.got.get(sym.name()?).context("GOT entry missing")?;
                    return Ok(Some(got_va + off));
                }
                Ok(Some(self.resolve_sym(file_idx, &sym)?))
            }
            RelocationTarget::Section(idx) => Ok(Some(self.section_address(file_idx, idx))),
            _ => Ok(None),
        }
    }

    fn resolve_sym(&self, file_idx: usize, sym: &object::Symbol) -> Result<u64> {
        if sym.kind() == SymbolKind::Section {
            let idx = sym.section_index().context("section symbol without index")?;
            return Ok(self.section_address(file_idx, idx));
        }

        if sym.is_local() {
            let base = sym.section_index()
                .map(|idx| self.section_address(file_idx, idx))
                .unwrap_or(0);
            return Ok(base + sym.address());
        }

        let name = sym.name()?;
        let addr = self.symbol_address(name);
        if addr == 0 && !self.weak_undefined.contains(name) && !is_optional_symbol(name) {
            if !self.symbols.contains_key(name) {
                return Err(anyhow!("undefined symbol: {}", name));
            }
        }
        Ok(addr)
    }

    fn symbol_address(&self, name: &str) -> u64 {
        if name == "_GLOBAL_OFFSET_TABLE_" {
            return self.got_address();
        }

        if let Some(sym) = self.symbols.get(name) {
            if let Some(addr) = sym.resolved_address {
                return addr;
            }
        }

        if self.weak_undefined.contains(name) || is_optional_symbol(name) {
            return 0;
        }

        0
    }

    fn section_address(&self, file_idx: usize, section_idx: SectionIndex) -> u64 {
        self.section_map.get(&(file_idx, section_idx))
            .map(|&(seg_idx, off)| self.segments[seg_idx].virtual_address + off)
            .unwrap_or(0)
    }

    fn got_address(&self) -> u64 {
        self.segments.iter().find(|s| s.name == ".got").map(|s| s.virtual_address).unwrap_or(0)
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Phase 3: Write output
    // ─────────────────────────────────────────────────────────────────────────

    pub fn write(&self, output: &PathBuf) -> Result<()> {
        let entry = self.symbol_address("_start");
        writer::write_elf(output, &self.segments, entry)
    }
}
