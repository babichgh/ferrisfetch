//! CPU usage, sampled only when explicitly selected.

use crate::fetch::Context;
use std::time::Duration;

pub const DEFAULT_INTERVAL: Duration = Duration::from_millis(200);

pub fn cpu_usage(ctx: &Context) -> Option<String> {
    format_usage(crate::fetch::cpu_usage(ctx)?)
}

fn format_usage(usage: f32) -> Option<String> {
    if !usage.is_finite() || usage < 0.0 {
        return None;
    }
    Some(format!("{:.0}%", usage.clamp(0.0, 100.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_and_clamps_percentage() {
        assert_eq!(format_usage(0.0).as_deref(), Some("0%"));
        assert_eq!(format_usage(12.6).as_deref(), Some("13%"));
        assert_eq!(format_usage(100.0).as_deref(), Some("100%"));
        assert_eq!(format_usage(101.0).as_deref(), Some("100%"));
    }

    #[test]
    fn invalid_usage_is_omitted() {
        for usage in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0] {
            assert_eq!(format_usage(usage), None);
        }
    }

    #[test]
    fn missing_sample_is_omitted() {
        let ctx = Context::collect(&[]);
        assert_eq!(cpu_usage(&ctx), None);
    }
}
