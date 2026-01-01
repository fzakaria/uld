//! ELF Static Linker
//!
//! 1. Load objects/archives, build symbol table
//! 2. Layout sections into segments
//! 3. Resolve symbol addresses
//! 4. Apply relocations
//! 5. Write ELF

use anyhow::{anyhow, Context, Result};
use memmap2::Mmap;
use object::read::{Object, ObjectSection, RelocationTarget, SectionIndex};
use object::{ObjectSymbol, Relocation, RelocationKind, SectionKind, SymbolKind, SymbolVisibility};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::arch::Architecture;
use crate::layout::{Section, Segment};
use crate::symbol::{is_optional_symbol, DefinedSymbol};
use crate::utils::align_up;
use crate::writer;

const PAGE_SIZE: u64 = 0x1000;
const BASE_ADDR: u64 = 0x400000;

pub struct Linker<'a, A: Architecture> {
    arch: A,
    objects: Vec<object::File<'a>>,
    symbols: HashMap<String, DefinedSymbol>,
    segments: Vec<Segment>,
    section_map: HashMap<(usize, SectionIndex), (usize, u64)>,
    got: HashMap<String, u64>,
    weak: HashSet<String>,      // symbols that can be 0
    undefined: HashSet<String>, // needed for archive linking
}

impl<'a, A: Architecture> Linker<'a, A> {
    pub fn new(arch: A) -> Self {
        Self {
            arch,
            objects: Vec::new(),
            symbols: HashMap::new(),
            segments: Vec::new(),
            section_map: HashMap::new(),
            got: HashMap::new(),
            weak: HashSet::new(),
            undefined: HashSet::new(),
        }
    }

    pub fn add_file(&mut self, path: &PathBuf, mmap: &'a Mmap) -> Result<()> {
        // https://alpha-supernova.dev.filibeto.org/lib/rel/5.1B/DOCS/HTML/SUPPDOCS/OBJSPEC/NV160XXX.HTM
        if mmap.starts_with(b"!<arch>\n") {
            return self.add_archive(path, mmap);
        }
        self.add_object(object::File::parse(&**mmap)?)
    }

    fn add_archive(&mut self, path: &PathBuf, mmap: &'a Mmap) -> Result<()> {
        let archive = object::read::archive::ArchiveFile::parse(mmap.as_ref())?;

        // Loop over all the object files within the archive
        // Create an index of symbol name -> archive member data
        let mut index: HashMap<String, &'a [u8]> = HashMap::new();
        for member in archive.members() {
            let member = member?;
            let mut data = member.data(mmap.as_ref())?;
            // Align for parsing
            if data.as_ptr().align_offset(8) != 0 {
                // Force the data onto the heap to get it aligned
                // FIXME: Can we avoid this leak?
                data = Box::leak(data.to_vec().into_boxed_slice());
            }
            let Ok(obj) = object::File::parse(data) else {
                tracing::info!(
                    "Failed to parse archive member {:?} within {:?}",
                    member,
                    path
                );
                continue;
            };
            // Kind of an edge case but maybe this archive contains different
            // architectures
            if obj.architecture() != A::arch() {
                continue;
            }
            for sym in obj.symbols() {
                let name = sym.name()?;
                if !sym.is_undefined() && !sym.is_local() {
                    index.insert(name.to_string(), data);
                }
            }
        }

        // FIXME: If we happen to parse archives before any object files the
        // needed list will be empty.
        // Pull in members defining needed symbols (iterate until fixpoint)
        let mut included = HashSet::new();
        loop {
            let needed: Vec<_> = self
                .undefined
                .iter()
                .filter(|s| index.contains_key(*s) && !included.contains(*s))
                .cloned()
                .collect();
            if needed.is_empty() {
                break;
            }
            for sym in needed {
                if let Some(&data) = index.get(&sym) {
                    included.insert(sym);
                    self.add_object(object::File::parse(data)?)?;
                }
            }
        }
        Ok(())
    }

    fn add_object(&mut self, obj: object::File<'a>) -> Result<()> {
        if A::arch() != obj.architecture() {
            return Err(anyhow!("unsupported: {:?}", obj.architecture()));
        }

        let idx = self.objects.len();

        for sym in obj.symbols() {
            let name = sym.name()?;

            if sym.is_undefined() {
                if sym.is_weak()
                    || sym.visibility() == SymbolVisibility::Hidden
                    || (sym.kind() == SymbolKind::Tls)
                    || is_optional_symbol(name)
                {
                    self.weak.insert(name.to_string());
                } else if !self.symbols.contains_key(name) {
                    self.undefined.insert(name.to_string());
                }
                continue;
            }

            if sym.is_local() {
                continue;
            }

            // If the symbol is weak, we actually let the next one overwrite it.
            if self.symbols.contains_key(name) && !self.symbols[name].is_weak {
                continue;
            }

            self.undefined.remove(name);
            self.symbols.insert(
                name.to_string(),
                DefinedSymbol::new(
                    idx,
                    sym.section_index().unwrap_or(SectionIndex(0)),
                    sym.address(),
                    sym.is_weak(),
                    sym.section_index().is_none(),
                ),
            );
        }

        self.objects.push(obj);
        Ok(())
    }

    pub fn link(&mut self) -> Result<()> {
        self.layout()?;
        self.resolve_symbols();
        self.relocate()
    }

    fn layout(&mut self) -> Result<()> {
        // BSS must be last (no file content)
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

        for (file_idx, obj) in self.objects.iter().enumerate() {
            for sec in obj.sections() {
                if sec.size() == 0 {
                    continue;
                }
                let Some(seg_idx) = self.segment_for(&sec) else {
                    continue;
                };

                let seg = &mut self.segments[seg_idx];
                let off = align_up(seg.size, sec.align().max(1));
                seg.size = off + sec.size();

                if sec.kind() != SectionKind::UninitializedData {
                    seg.data.resize(off as usize, 0);
                    seg.data.extend_from_slice(sec.data()?);
                }

                seg.sections.push(Section {
                    file_index: file_idx,
                    section_index: sec.index(),
                    offset: off,
                });
                self.section_map
                    .insert((file_idx, sec.index()), (seg_idx, off));
            }
        }

        self.build_got()?;

        // Assign addresses
        let (mut va, mut fo) = (BASE_ADDR + PAGE_SIZE, PAGE_SIZE);
        for seg in &mut self.segments {
            if seg.size == 0 {
                continue;
            }
            va = align_up(va, PAGE_SIZE);
            fo = align_up(fo, PAGE_SIZE);
            seg.virtual_address = va;
            seg.file_offset = fo;
            va += seg.size;
            if seg.kind != SectionKind::UninitializedData {
                fo += seg.size;
            }
        }
        Ok(())
    }

    /// Which segment should this section go into?
    fn segment_for(&self, sec: &object::Section) -> Option<usize> {
        match sec.name().unwrap_or("") {
            ".init" => Some(1),
            ".fini" => Some(2),
            _ => match sec.kind() {
                SectionKind::Text => Some(0),
                SectionKind::ReadOnlyData | SectionKind::ReadOnlyString => Some(3),
                SectionKind::Data | SectionKind::Elf(14) | SectionKind::Elf(15) => Some(4),
                SectionKind::Tls => Some(6),
                SectionKind::UninitializedData => Some(7),
                _ => {
                    tracing::debug!("Skip: {} ({:?})", sec.name().unwrap_or("?"), sec.kind());
                    None
                }
            },
        }
    }

    fn build_got(&mut self) -> Result<()> {
        let mut off = 0u64;
        for obj in &self.objects {
            for sec in obj.sections() {
                for (_, r) in sec.relocations() {
                    let needs =
                        matches!(r.kind(), RelocationKind::Got | RelocationKind::GotRelative)
                            || matches!(r.target(), RelocationTarget::Symbol(i)
                            if obj.symbol_by_index(i).is_ok_and(|s| s.kind() == SymbolKind::Tls));
                    if !needs {
                        continue;
                    }
                    let RelocationTarget::Symbol(i) = r.target() else {
                        continue;
                    };
                    let name = obj.symbol_by_index(i)?.name()?;
                    if !self.got.contains_key(name) {
                        self.got.insert(name.to_string(), off);
                        off += 8;
                    }
                }
            }
        }
        if let Some(g) = self.segments.iter_mut().find(|s| s.name == ".got") {
            g.size = off;
            g.data.resize(off as usize, 0);
        }
        Ok(())
    }

    fn resolve_symbols(&mut self) {
        for sym in self.symbols.values_mut() {
            sym.resolved_address = if sym.is_absolute {
                Some(sym.offset)
            } else if let Some(&(si, o)) = self
                .section_map
                .get(&(sym.input_file_index, sym.section_index))
            {
                Some(self.segments[si].virtual_address + o + sym.offset)
            } else {
                None
            };
        }
    }

    fn relocate(&mut self) -> Result<()> {
        // Fill GOT
        let entries: Vec<_> = self
            .got
            .iter()
            .map(|(name, &offset)| (offset, self.sym_addr(name)))
            .collect();
        if let Some(g) = self.segments.iter_mut().find(|s| s.name == ".got") {
            for (offset, addr) in entries {
                g.data[offset as usize..][..8].copy_from_slice(&addr.to_le_bytes());
            }
        }

        // Apply relocations
        let got_va = self.got_addr();
        for si in 0..self.segments.len() {
            let patches: Vec<_> = self.segments[si]
                .sections
                .clone()
                .iter()
                .flat_map(|sec| {
                    let obj = &self.objects[sec.file_index];
                    let s = obj.section_by_index(sec.section_index).ok()?;
                    let base = self.segments[si].virtual_address + sec.offset;
                    Some(
                        s.relocations()
                            .filter_map(|(o, r)| {
                                let t =
                                    self.reloc_target(obj, &r, sec.file_index, got_va).ok()?;
                                Some((sec.offset + o, r, base + o, t))
                            })
                            .collect::<Vec<_>>(),
                    )
                })
                .flatten()
                .collect();

            for (o, r, p, t) in patches {
                self.arch
                    .apply_relocation(o, &r, p, t, r.addend(), &mut self.segments[si].data)?;
            }
        }
        Ok(())
    }

    /// Find the address of a relocation target
    /// Afterwards the arch specific implementation can apply the relocation
    fn reloc_target(&self, obj: &object::File, r: &Relocation, fi: usize, got: u64) -> Result<u64> {
        Ok(match r.target() {
            RelocationTarget::Symbol(i) => {
                let s = obj.symbol_by_index(i)?;
                let use_got = matches!(r.kind(), RelocationKind::Got | RelocationKind::GotRelative)
                    || s.kind() == SymbolKind::Tls;
                if use_got {
                    let name = s.name()?;
                    got + self.got.get(name).context(format!("Missing GOT entry for: {}", name))?
                } else {
                    self.resolve_sym(fi, &s)?
                }
            }
            RelocationTarget::Section(i) => self.sec_addr(fi, i),
            RelocationTarget::Absolute => 0,
            _ => unreachable!("This target never existed before: {:?}", r.target()),
        })
    }

    fn resolve_sym(&self, fi: usize, s: &object::Symbol) -> Result<u64> {
        if s.kind() == SymbolKind::Section {
            return Ok(self.sec_addr(fi, s.section_index().context("no section")?));
        }
        if s.is_local() {
            let base = s.section_index().map(|i| self.sec_addr(fi, i)).unwrap_or(0);
            return Ok(base + s.address());
        }
        let name = s.name()?;
        let addr = self.sym_addr(name);
        if addr == 0
            && !self.weak.contains(name)
            && !is_optional_symbol(name)
            && !self.symbols.contains_key(name)
        {
            return Err(anyhow!("undefined: {}", name));
        }
        Ok(addr)
    }

    fn sym_addr(&self, name: &str) -> u64 {
        if name == "_GLOBAL_OFFSET_TABLE_" {
            return self.got_addr();
        }
        self.symbols
            .get(name)
            .and_then(|s| s.resolved_address)
            .unwrap_or(0)
    }

    fn sec_addr(&self, fi: usize, si: SectionIndex) -> u64 {
        self.section_map
            .get(&(fi, si))
            .map(|&(i, o)| self.segments[i].virtual_address + o)
            .unwrap_or(0)
    }

    fn got_addr(&self) -> u64 {
        self.segments
            .iter()
            .find(|s| s.name == ".got")
            .map(|s| s.virtual_address)
            .unwrap_or(0)
    }

    pub fn write(&self, out: &PathBuf) -> Result<()> {
        writer::write_elf(out, &self.segments, self.sym_addr("_start"))
    }
}
