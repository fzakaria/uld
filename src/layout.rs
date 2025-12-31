//! Layout management.
//!
//! This module defines the structures for organizing the output executable's memory layout.
//! It maps sections from input files into aggregated segments (e.g., .text, .data).

use object::read::SectionIndex;
use object::SectionKind;

/// Represents a section from an input file.
///
/// A `Section` corresponds to a section from an object file that will be copied
/// into a specific `Segment`.
pub struct Section {
    /// Index of the input file.
    pub file_index: usize,
    /// Index of the section in the input file.
    pub section_index: SectionIndex,
    /// The offset where this section starts within the `Segment`.
    pub offset: u64, 
}

/// Represents a segment in the final output executable.
///
/// A `Segment` aggregates multiple input `Section`s of the same type (e.g., all .text sections).
/// It tracks the total size, the virtual address where it will be loaded, and the raw data bytes.
pub struct Segment {
    /// Name of the segment (e.g., ".text", ".data").
    pub name: String,
    /// List of input sections that make up this segment.
    pub sections: Vec<Section>,
    /// Total size of the segment in bytes.
    pub size: u64,
    /// The virtual address where this segment starts in memory.
    pub virtual_address: u64,
    /// The file offset where this segment starts in the ELF file.
    pub file_offset: u64,
    /// The raw data content of the segment.
    pub data: Vec<u8>, 
    /// The kind of segment (Text, Data, etc.) used for permissions and mapping.
    pub kind: SectionKind,
}

impl Segment {
    /// Creates a new, empty segment.
    pub fn new(name: &str, kind: SectionKind) -> Self {
        Self {
            name: name.to_string(),
            sections: Vec::new(),
            size: 0,
            virtual_address: 0,
            file_offset: 0,
            data: Vec::new(),
            kind,
        }
    }
}