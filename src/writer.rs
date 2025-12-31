//! ELF file writer.
//!
//! This module handles writing the final ELF executable file.

use anyhow::Result;
use object::endian::{U16, U32, U64};
use object::pod::bytes_of;
use object::{Endianness, SectionKind};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use crate::layout::Segment;

const PAGE_SIZE: u64 = 0x1000;
const BASE_ADDR: u64 = 0x400000;

fn u16(v: u16) -> U16<Endianness> {
    U16::new(Endianness::Little, v)
}
fn u32(v: u32) -> U32<Endianness> {
    U32::new(Endianness::Little, v)
}
fn u64(v: u64) -> U64<Endianness> {
    U64::new(Endianness::Little, v)
}

/// Write an ELF executable to disk.
pub fn write_elf(
    output_path: &PathBuf,
    segments: &[Segment],
    entry_point: u64,
) -> Result<()> {
    let mut buffer = Vec::new();
    let num_sections = segments.len() as u32 + 2;

    // ELF file header
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
        e_entry: u64(entry_point),
        e_phoff: u64(64),
        e_shoff: u64(0), // Will be patched later
        e_flags: u32(0),
        e_ehsize: u16(64),
        e_phentsize: u16(56),
        e_phnum: u16(1),
        e_shentsize: u16(64),
        e_shnum: u16(num_sections as u16),
        e_shstrndx: u16(num_sections as u16 - 1),
    };
    buffer.extend_from_slice(bytes_of(&file_header));

    // Calculate file and memory sizes for the LOAD segment
    let last_segment = segments
        .iter()
        .filter(|s| s.kind != SectionKind::UninitializedData && s.size > 0)
        .last();

    let file_size = if let Some(seg) = last_segment {
        seg.file_offset + seg.size
    } else {
        PAGE_SIZE
    };

    let mem_size = segments
        .iter()
        .map(|s| {
            if s.virtual_address > 0 {
                s.virtual_address + s.size
            } else {
                BASE_ADDR
            }
        })
        .max()
        .unwrap_or(BASE_ADDR)
        - BASE_ADDR;

    // Single LOAD program header
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

    // Pad to first page boundary
    if (buffer.len() as u64) < PAGE_SIZE {
        buffer.resize(PAGE_SIZE as usize, 0);
    }

    // Write segment data
    for segment in segments {
        if segment.kind == SectionKind::UninitializedData {
            continue;
        }
        let current = buffer.len() as u64;
        if segment.file_offset > current {
            buffer.resize(segment.file_offset as usize, 0);
        }
        buffer.extend_from_slice(&segment.data);
    }

    // Build section header string table
    let mut shstrtab = Vec::new();
    shstrtab.push(0);
    let mut section_name_offsets = Vec::new();
    section_name_offsets.push(0);

    for segment in segments {
        let off = shstrtab.len();
        section_name_offsets.push(off);
        shstrtab.extend_from_slice(segment.name.as_bytes());
        shstrtab.push(0);
    }

    let shstrtab_offset = shstrtab.len();
    section_name_offsets.push(shstrtab_offset);
    shstrtab.extend_from_slice(b".shstrtab\0");

    let shoff = buffer.len();

    // Null section header
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

    // Section headers for each segment
    for (i, segment) in segments.iter().enumerate() {
        let sec_header = object::elf::SectionHeader64::<Endianness> {
            sh_name: u32(section_name_offsets[i + 1] as u32),
            sh_type: u32(if segment.kind == SectionKind::UninitializedData {
                object::elf::SHT_NOBITS
            } else {
                object::elf::SHT_PROGBITS
            }),
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

    // Section header string table header
    let shstrtab_header = object::elf::SectionHeader64::<Endianness> {
        sh_name: u32(section_name_offsets[section_name_offsets.len() - 1] as u32),
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

    // String table contents
    buffer.extend_from_slice(&shstrtab);

    // Patch e_shoff in the file header
    let shoff_bytes = (shoff as u64).to_le_bytes();
    buffer[40..48].copy_from_slice(&shoff_bytes);

    // Write file
    std::fs::write(output_path, &buffer)?;

    // Make executable
    let mut perms = std::fs::metadata(output_path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(output_path, perms)?;

    Ok(())
}
