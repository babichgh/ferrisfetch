//! CPU model, core count and frequency (no usage sampling).

use crate::fetch::Context;

/// Strips the clock suffix vendors bake into their model strings.
///
/// `/proc/cpuinfo` reports e.g. `Intel(R) Xeon(R) CPU E5-2680 v3 @ 2.50GHz`;
/// keeping it would duplicate the frequency we append ourselves.
pub fn clean_brand(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(pos) = trimmed.to_ascii_lowercase().rfind(" @ ") {
        let tail = &trimmed[pos + 3..];
        if tail.to_ascii_lowercase().ends_with("ghz") {
            return trimmed[..pos].trim_end().to_string();
        }
    }
    trimmed.to_string()
}

/// e.g. `AMD Ryzen 7 5800X (8) @ 4.90 GHz`.
pub fn cpu(ctx: &Context) -> Option<String> {
    let system = crate::fetch::system(ctx);
    let cpus = system.cpus();
    let first = cpus.first()?;

    let brand = clean_brand(first.brand());
    if brand.is_empty() {
        return None;
    }

    let mut parts = vec![brand];

    let cores = sysinfo::System::physical_core_count().unwrap_or(cpus.len());
    if cores > 0 {
        parts.push(format!("({cores})"));
    }

    let freq_mhz = first.frequency();
    if freq_mhz > 0 {
        // Fixed two decimals: trimming would turn "2.50" into a bare "2.5".
        parts.push(format!("@ {:.2} GHz", freq_mhz as f64 / 1000.0));
    }

    Some(parts.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetch::Module;

    #[test]
    fn resolves_on_this_machine() {
        let ctx = Context::collect(&[Module::Cpu]);
        let v = cpu(&ctx).expect("cpu should resolve");
        assert!(!v.is_empty());
    }

    #[test]
    fn brand_drops_embedded_clock() {
        assert_eq!(
            clean_brand("Intel(R) Xeon(R) CPU E5-2680 v3 @ 2.50GHz"),
            "Intel(R) Xeon(R) CPU E5-2680 v3"
        );
        assert_eq!(
            clean_brand("AMD Ryzen 7 5800X 8-Core Processor @ 3.70GHz"),
            "AMD Ryzen 7 5800X 8-Core Processor"
        );
    }

    #[test]
    fn brand_keeps_names_without_clock() {
        assert_eq!(clean_brand("Apple M1 Pro"), "Apple M1 Pro");
        assert_eq!(clean_brand("  Intel i7  "), "Intel i7");
        // '@' without a GHz suffix is part of the name, not a clock.
        assert_eq!(clean_brand("A @ B"), "A @ B");
    }

    #[test]
    fn cpu_info_does_not_include_usage() {
        let ctx = Context::collect(&[Module::Cpu]);
        let v = cpu(&ctx).unwrap();
        assert!(!v.contains('%'), "unexpected usage: {v}");
    }
}
