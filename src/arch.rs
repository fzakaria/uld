//! Architecture abstraction.
//!
//! This module defines the `Architecture` trait, which encapsulates all architecture-specific logic.
//! This allows the core linker to remain generic while specific backends handle details like
//! relocation types and ELF header formats.

use anyhow::Result;
use object::read::Relocation;
use object::Endianness;

pub mod x86_64;

/// A trait representing a target architecture (e.g., x86_64, AArch64).
pub trait Architecture {
    /// The object crate's endianness for this architecture.
    fn endianness(&self) -> Endianness;

    /// Applies a relocation to a buffer.
    ///
    /// # Arguments
    /// * `offset` - The offset within the buffer where the relocation should be applied.
    /// * `reloc` - The relocation entry from the input object file.
    /// * `p` - The runtime address of the location being relocated (P).
    /// * `s` - The value of the symbol (S).
    /// * `a` - The addend (A).
    /// * `data` - The mutable buffer representing the section's data.
    fn apply_relocation(
        &self,
        offset: u64,
        reloc: &Relocation,
        p: u64,
        s: u64,
        a: i64,
        data: &mut [u8],
    ) -> Result<()>;
}