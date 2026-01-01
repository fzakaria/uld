//! Utility functions.

use std::path::PathBuf;

/// Aligns an address up to the next multiple of `align`.
pub fn align_up(addr: u64, align: u64) -> u64 {
    (addr + align - 1) & !(align - 1)
}

/// Find `lib{name}.a` in search paths.
pub fn find_library(name: &str, paths: &[PathBuf]) -> Option<PathBuf> {
    let filename = format!("lib{}.a", name);
    paths.iter().map(|p| p.join(&filename)).find(|p| p.exists())
}
