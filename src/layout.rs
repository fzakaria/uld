//! Layout management.
//!
//! This module defines the structures for organizing the output executable's memory layout.
//! It maps chunks of code/data from input files into aggregated output sections (e.g., .text, .data).

use object::read::SectionIndex;
use object::SectionKind;

/// Represents a piece of code or data from an input file.
///
/// An `InputChunk` corresponds to a section from an object file that will be copied
/// into a specific `OutputSection`.
pub struct InputChunk {
    /// Index of the input file.
    pub file_index: usize,
    /// Index of the section in the input file.
    pub section_index: SectionIndex,
    /// The offset where this chunk starts within the `OutputSection`.
    pub offset: u64, 
}

/// Represents a section in the final output executable.
///
/// An `OutputSection` aggregates multiple `InputChunk`s of the same type (e.g., all .text sections).
/// It tracks the total size, the virtual address where it will be loaded, and the raw data bytes.
pub struct OutputSection {
    /// Name of the section (e.g., ".text", ".data").
    pub name: String,
    /// List of input chunks that make up this section.
    pub chunks: Vec<InputChunk>,
    /// Total size of the section in bytes.
    pub size: u64,
    /// The virtual address where this section starts in memory.
    pub virtual_address: u64,
    /// The file offset where this section starts in the ELF file.
    pub file_offset: u64,
    /// The raw data content of the section.
    pub data: Vec<u8>, 
    /// The kind of section (Text, Data, etc.) used for permissions and mapping.
    pub kind: SectionKind,
}

impl OutputSection {
    /// Creates a new, empty output section.
    pub fn new(name: &str, kind: SectionKind) -> Self {
        Self {
            name: name.to_string(),
            chunks: Vec::new(),
            size: 0,
            virtual_address: 0,
            file_offset: 0,
            data: Vec::new(),
            kind,
        }
    }
}
