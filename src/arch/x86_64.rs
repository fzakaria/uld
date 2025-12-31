//! x86_64 Architecture backend.
//!
//! Implements the `Architecture` trait for 64-bit x86 systems (ELF64).

use super::Architecture;
use anyhow::{anyhow, Result};
use object::read::Relocation;
use object::{Endianness, RelocationKind};

/// The x86_64 architecture backend.
pub struct X86_64;

impl Architecture for X86_64 {
    fn endianness(&self) -> Endianness {
        Endianness::Little
    }

    fn apply_relocation(
        &self,
        offset: u64,
        reloc: &Relocation,
        p: u64, // Place of storage (P)
        s: u64, // Symbol value or GOT entry VA (S)
        a: i64, // Addend (A)
        data: &mut [u8],
    ) -> Result<()> {
        let offset = offset as usize;

        // Calculate the value based on the relocation formula.
        // P: Place of storage (chunk_va + offset)
        // S: Value of the symbol or GOT entry
        // A: Addend
        let val: u64 = match reloc.kind() {
            // R_X86_64_64: S + A
            RelocationKind::Absolute => (s as i64 + a) as u64,

            // R_X86_64_PC32 / PLT32 / GOTPCREL: S + A - P
            RelocationKind::Relative | RelocationKind::PltRelative | RelocationKind::GotRelative => {
                (s as i64 + a - p as i64) as u64
            }
            _ => return Ok(()), // Skip unsupported relocations
        };

        // Write the value to the buffer.
        match reloc.size() {
            32 => {
                let bytes = (val as u32).to_le_bytes();
                if offset + 4 <= data.len() {
                    data[offset..offset + 4].copy_from_slice(&bytes);
                }
            }
            64 => {
                let bytes = val.to_le_bytes();
                if offset + 8 <= data.len() {
                    data[offset..offset + 8].copy_from_slice(&bytes);
                }
            }
            _ => return Err(anyhow!("Unsupported relocation size: {}", reloc.size())),
        }

        Ok(())
    }
}