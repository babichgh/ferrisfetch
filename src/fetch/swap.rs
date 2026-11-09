//! Optional swap usage, sharing the memory refresh with RAM.

use crate::fetch::Context;
use crate::fetch::util::bytes;

pub fn swap(ctx: &Context) -> Option<String> {
    let system = crate::fetch::system(ctx);
    format_usage(system.used_swap(), system.total_swap())
}

fn format_usage(used: u64, total: u64) -> Option<String> {
    if total == 0 {
        return None;
    }
    let used = used.min(total);
    let percent = (used as f64 / total as f64 * 100.0).clamp(0.0, 100.0);
    Some(format!(
        "{} / {} ({percent:.0}%)",
        bytes(used),
        bytes(total)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_usage_with_given_numbers() {
        assert_eq!(
            format_usage(1 << 30, 4 << 30).unwrap(),
            "1.0 GiB / 4.0 GiB (25%)"
        );
        assert_eq!(format_usage(0, 1 << 30).unwrap(), "0 B / 1.0 GiB (0%)");
        assert_eq!(format_usage(1, 1).unwrap(), "1 B / 1 B (100%)");
    }

    #[test]
    fn zero_total_is_omitted() {
        assert!(format_usage(0, 0).is_none());
        assert!(format_usage(u64::MAX, 0).is_none());
    }

    #[test]
    fn inconsistent_and_extreme_counters_are_bounded() {
        assert_eq!(format_usage(u64::MAX, 1).unwrap(), "1 B / 1 B (100%)");
        assert!(
            format_usage(u64::MAX, u64::MAX)
                .unwrap()
                .ends_with("(100%)")
        );
        assert!(format_usage(0, u64::MAX).unwrap().ends_with("(0%)"));
    }
}
