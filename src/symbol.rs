//! Symbol management.
//!
//! This module defines structures for tracking symbols found in input object files.
//! It handles the global symbol table, which maps symbol names to their definitions
//! (file index, section index, value).

use object::read::SectionIndex;

/// Represents a defined symbol within the linker's global view.
///
/// A `DefinedSymbol` points to the specific input file and section where the symbol
/// is defined, along with its offset (value) within that section.
#[derive(Debug, Clone, Copy)]
pub struct DefinedSymbol {
    /// Index of the input file in the linker's file list.
    pub input_file_index: usize,
    /// Index of the section within that input file.
    pub section_index: SectionIndex,
    /// The value of the symbol relative to the section start.
    pub value: u64,
    /// Whether the symbol is weak (can be overridden by a strong symbol).
    pub is_weak: bool,
}
