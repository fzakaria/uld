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
        p: u64, // Place of storage (P) - The VA where the relocation is written
        s: u64, // Symbol value OR GOT entry VA (S)
        a: i64, // Addend (A)
        data: &mut [u8],
    ) -> Result<()> {
        let offset = offset as usize;

        // x86_64 often uses SHT_RELA (explicit addends), but we should check
        // if the addend is 0 and try to read from the buffer to support implicit addends
        // which can sometimes appear in specific toolchains or object formats.
        let mut final_addend = a;
        if final_addend == 0 && reloc.size() == 32
            && offset + 4 <= data.len() {
                let existing = i32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
                if existing != 0 {
                    final_addend = existing as i64;
                }
            }

        let val: u64 = match reloc.kind() {
            // R_X86_64_64: S + A
            RelocationKind::Absolute => (s as i64 + final_addend) as u64,

            // R_X86_64_PC32 / PLT32 / GOTPCREL / GOTPCRELX: S + A - P
            RelocationKind::Relative
            | RelocationKind::PltRelative
            | RelocationKind::GotRelative => (s as i64 + final_addend - p as i64) as u64,

            _ => {
                tracing::trace!("Unsupported relocation kind: {:?}", reloc.kind());
                return Ok(());
            }
        };

        // Write the value to the buffer.
        match reloc.size() {
            32 => {
                // x86_64 PC-relative displacements are signed 32-bit integers.
                let signed_val = val as i64;
                if signed_val < i32::MIN as i64 || signed_val > i32::MAX as i64 {
                    return Err(anyhow!(
                        "Relocation overflow at VA 0x{:x}: displacement 0x{:x} exceeds 32-bit signed range. \
                         Target (S) is 0x{:x}, P is 0x{:x}. Ensure segments are within 2GB of each other.",
                        p, signed_val, s, p
                    ));
                }

                let bytes = (val as u32).to_le_bytes();
                if offset + 4 <= data.len() {
                    data[offset..offset + 4].copy_from_slice(&bytes);
                } else {
                    return Err(anyhow!("Relocation offset out of bounds at 0x{:x}", offset));
                }
            }
            64 => {
                let bytes = val.to_le_bytes();
                if offset + 8 <= data.len() {
                    data[offset..offset + 8].copy_from_slice(&bytes);
                } else {
                    return Err(anyhow!("Relocation offset out of bounds at 0x{:x}", offset));
                }
            }
            _ => return Err(anyhow!("Unsupported relocation size: {}", reloc.size())),
        }

        Ok(())
    }
}
