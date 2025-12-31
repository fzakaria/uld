//! Utility functions.

/// Aligns an address or size up to the next multiple of `align`.
/// `align` must be a power of two.
pub fn align_up(addr: u64, align: u64) -> u64 {
    assert!(align.is_power_of_two());
    if align == 0 {
        return addr;
    }
    (addr + align - 1) & !(align - 1)
}
