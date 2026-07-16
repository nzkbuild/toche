//! Global truncation caps shared by every filter. Vendored from RTK.

/// Errors: most actionable, shown the most.
pub const CAP_ERRORS: usize = 20;
/// Warnings and test failures: lower signal density than errors.
pub const CAP_WARNINGS: usize = 10;
/// Flat lists (PRs, services, packages): one line per item.
pub const CAP_LIST: usize = 20;
/// Inventories (`pip list`, `docker images`): exhaustive lookups.
pub const CAP_INVENTORY: usize = 50;

/// A cap reduced for a verbose data class. Falls back to `cap` when `by >= cap`
/// so a deviation can never empty the list; `0` stays `0`. `const fn`, underflow-safe.
pub const fn reduced(cap: usize, by: usize) -> usize {
    if by < cap {
        cap - by
    } else {
        cap
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduced_preserves_current_values() {
        assert_eq!(reduced(CAP_WARNINGS, 5), 5);
        assert_eq!(reduced(CAP_LIST, 5), 15);
    }

    #[test]
    fn reduced_falls_back_to_cap_when_offset_too_large() {
        assert_eq!(reduced(4, 5), 4);
        assert_eq!(reduced(5, 5), 5);
    }

    #[test]
    fn reduced_honors_zero_cap() {
        assert_eq!(reduced(0, 5), 0);
    }
}
