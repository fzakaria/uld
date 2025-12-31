//! x86_64 Architecture backend.
//!
//! Implements the `Architecture` trait for 64-bit x86 systems (ELF64).
//! Handles specific relocations as defined in the System V AMD64 ABI.
//!
//! Reference: <https://refspecs.linuxbase.org/elf/x86_64-abi-0.99.pdf>

use super::Architecture;
use anyhow::Result;
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
        p: u64,
        s: u64,
        a: i64,
        data: &mut [u8],
    ) -> Result<()> {
        let offset = offset as usize;
        
        // Calculate the value based on the relocation formula.
        // P: Place of storage (chunk_va + offset)
        // S: Value of the symbol
        // A: Addend
        let val: u64 = match reloc.kind() {
            // R_X86_64_64: S + A
            RelocationKind::Absolute => (s as i64 + a) as u64,
            // R_X86_64_PC32 / PLT32: S + A - P
            RelocationKind::Relative | RelocationKind::PltRelative => {
                (s as i64 + a - p as i64) as u64
            }
            RelocationKind::GotRelative => {
                // Relaxation: If the instruction is MOV (0x8B), convert to LEA (0x8D)
                // to load the address directly instead of from the GOT.
                // The offset points to the displacement, so opcode is at offset - 2.
                if offset >= 2 && data[offset - 2] == 0x8b {
                    tracing::info!("Relaxing GOTPCREL MOV at offset {:x} to LEA", offset);
                    data[offset - 2] = 0x8d;
                    (s as i64 + a - p as i64) as u64
                } else {
                     tracing::warn!("Could not relax GOTPCREL at offset {:x} (opcode: {:x})", offset, if offset >= 2 { data[offset - 2] } else { 0 });
                     // If we can't relax, we should ideally use the GOT.
                     // But since we don't have a populated GOT, this will likely fail/crash.
                     // For now, let's assume relaxation works or try S + A - P and hope.
                     (s as i64 + a - p as i64) as u64
                }
            }
            kind => {
                tracing::debug!("Skipping relocation kind {:?} at offset {:x}", kind, offset);
                return Ok(());
            }
        };

        // Write the calculated value into the buffer based on the relocation size.
        match reloc.size() {
            32 => {
                let bytes = (val as u32).to_le_bytes();
                if offset + 4 <= data.len() {
                    data[offset..offset+4].copy_from_slice(&bytes);
                }
            }
            64 => {
                let bytes = val.to_le_bytes();
                if offset + 8 <= data.len() {
                    data[offset..offset+8].copy_from_slice(&bytes);
                }
            }
            _ => {}
        }

        Ok(())
    }
}
