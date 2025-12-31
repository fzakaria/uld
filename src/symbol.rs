//! Symbol table management.
//!
//! Tracks symbols from input object files and resolves them to final addresses.

use object::read::SectionIndex;

/// A symbol defined in an input object file.
///
/// Initially stores indices for deferred address resolution.
/// After layout, `resolved_address` is populated with the final virtual address.
#[derive(Debug, Clone, Copy)]
pub struct DefinedSymbol {
    /// Index of the input file in the linker's file list.
    pub input_file_index: usize,
    /// Section index within that input file.
    pub section_index: SectionIndex,
    /// Offset within the section (or absolute address if `is_absolute`).
    pub offset: u64,
    /// Whether this is a weak symbol (can be overridden).
    pub is_weak: bool,
    /// Whether this is an absolute symbol (not section-relative).
    pub is_absolute: bool,
    /// Final virtual address (populated after layout).
    pub resolved_address: Option<u64>,
}

impl DefinedSymbol {
    pub fn new(
        input_file_index: usize,
        section_index: SectionIndex,
        offset: u64,
        is_weak: bool,
        is_absolute: bool,
    ) -> Self {
        Self {
            input_file_index,
            section_index,
            offset,
            is_weak,
            is_absolute,
            resolved_address: None,
        }
    }

    /// Get the resolved address, panics if not yet resolved.
    pub fn address(&self) -> u64 {
        self.resolved_address.expect("symbol not yet resolved")
    }
}

/// Known symbols that can remain undefined (resolve to 0).
pub fn is_optional_symbol(name: &str) -> bool {
    matches!(name,
        "_DYNAMIC" | "__dso_handle" | "_dl_find_object" | "__TMC_END__"
    ) || name.starts_with("__TMC_")
      || name.starts_with("__gcc_")
}
