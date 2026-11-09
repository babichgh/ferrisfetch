//! GPU detection.
//!
//! Tries `nvidia-smi` first (richest output), then falls back to `lspci` for
//! Intel/AMD cards, then to `/sys/class/drm`. Returns `None` when no adapter
//! can be identified rather than guessing.

use crate::fetch::Context;
use crate::fetch::util::bytes;

pub fn gpu(_ctx: &Context) -> Option<String> {
    if let Some(v) = via_nvidia_smi() {
        return Some(v);
    }
    via_lspci().or_else(via_sysfs_drm)
}

/// Parses `name,memory.total` CSV output from `nvidia-smi`.
///
/// Input lines look like `NVIDIA GeForce RTX 3080, 10240`.
fn parse_nvidia_csv(stdout: &str) -> Option<String> {
    let mut cards = Vec::new();
    for line in stdout.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let mut parts = line.split(',').map(str::trim);
        let name = parts.next()?;
        if name.is_empty() || name.eq_ignore_ascii_case("[N/A]") {
            continue;
        }
        let card = match parts.next().and_then(|m| m.parse::<u64>().ok()) {
            Some(mib) => format!("{name} ({})", bytes(mib * 1024 * 1024)),
            None => name.to_string(),
        };
        cards.push(card);
    }

    match cards.len() {
        0 => None,
        1 => Some(cards.remove(0)),
        n => Some(format!("{}x {}", n, cards.join(" + "))),
    }
}

fn via_nvidia_smi() -> Option<String> {
    let stdout = super::command::output(
        "nvidia-smi",
        &[
            "--query-gpu=name,memory.total",
            "--format=csv,noheader,nounits",
        ],
    )?;
    parse_nvidia_csv(&stdout)
}

/// Extracts display controller names from `lspci` output.
fn parse_lspci(stdout: &str) -> Option<String> {
    let mut names = Vec::new();
    for line in stdout.lines() {
        let lower = line.to_ascii_lowercase();
        if !lower.contains("vga compatible controller")
            && !lower.contains("3d controller")
            && !lower.contains("display controller")
        {
            continue;
        }
        let Some((_, desc)) = line.split_once(": ") else {
            continue;
        };
        // Bracketed text often contains the actual GPU model (e.g.
        // "GA102 [GeForce RTX 3090]"); dropping it loses useful data.
        let desc = desc.trim();
        if !desc.is_empty() {
            names.push(desc.to_string());
        }
    }
    match names.len() {
        0 => None,
        1 => Some(names.remove(0)),
        n => Some(format!("{}x {}", n, names.join(" + "))),
    }
}

fn via_lspci() -> Option<String> {
    parse_lspci(&super::command::output("lspci", &[])?)
}

/// Minimal fallback: map the DRM vendor id to a vendor name.
fn parse_drm_vendor(vendor_hex: &str) -> Option<&'static str> {
    // Whitespace first: the sysfs file ends with a newline.
    let hex = vendor_hex.trim().to_ascii_lowercase();
    match hex.strip_prefix("0x").unwrap_or(&hex) {
        "10de" => Some("NVIDIA"),
        "1002" | "1022" => Some("AMD"),
        "8086" => Some("Intel"),
        "1a03" | "1af4" | "b8ce" => Some("Matrox/Other"),
        _ => None,
    }
}

fn via_sysfs_drm() -> Option<String> {
    let mut names: Vec<String> = Vec::new();
    for entry in std::fs::read_dir("/sys/class/drm").ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        // Skip connectors (`card0-DP-1`) and keep the card nodes.
        if !name.starts_with("card") || name.contains('-') {
            continue;
        }
        let vendor_path = entry.path().join("device/vendor");
        if let Ok(v) = std::fs::read_to_string(&vendor_path)
            && let Some(vendor) = parse_drm_vendor(&v)
        {
            names.push(format!("{vendor} GPU"));
        }
    }
    names.sort();
    names.dedup();
    match names.len() {
        0 => None,
        1 => Some(names.remove(0)),
        n => Some(format!("{n}x {}", names.join(" + "))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_nvidia_card() {
        let out = "NVIDIA GeForce RTX 3080, 10240\n";
        let v = parse_nvidia_csv(out).unwrap();
        assert!(v.starts_with("NVIDIA GeForce RTX 3080"));
        assert!(v.contains("GiB"), "got {v}");
    }

    #[test]
    fn collapses_duplicate_nvidia_cards() {
        let out = "NVIDIA RTX 3060, 12288\nNVIDIA RTX 3060, 12288\n";
        assert!(parse_nvidia_csv(out).unwrap().starts_with("2x"));
    }

    #[test]
    fn nvidia_csv_handles_missing_memory() {
        assert_eq!(
            parse_nvidia_csv("Some GPU, [N/A]\n"),
            Some("Some GPU".to_string())
        );
    }

    #[test]
    fn empty_nvidia_csv_is_none() {
        assert_eq!(parse_nvidia_csv(""), None);
        assert_eq!(parse_nvidia_csv("\n\n  \n"), None);
        assert_eq!(parse_nvidia_csv("[N/A], 100\n"), None);
    }

    #[test]
    fn parses_lspci_display_controllers() {
        let out = "00:02.0 VGA compatible controller: Intel Corporation UHD Graphics 770 (rev 02)\n\
                   01:00.0 3D controller: NVIDIA Corporation GA102 [GeForce RTX 3090]\n";
        let v = parse_lspci(out).unwrap();
        assert!(v.starts_with("2x"), "got {v}");
        assert!(v.contains("Intel"));
        assert!(v.contains("GeForce RTX 3090"), "model missing: {v}");
    }

    #[test]
    fn lspci_ignores_other_devices() {
        let out = "00:00.0 Host bridge: Intel Corporation 12th Gen Core Host Bridge\n";
        assert_eq!(parse_lspci(out), None);
    }

    #[test]
    fn maps_drm_vendor_ids() {
        assert_eq!(parse_drm_vendor("0x10de"), Some("NVIDIA"));
        assert_eq!(parse_drm_vendor("10DE"), Some("NVIDIA"));
        assert_eq!(parse_drm_vendor("0x8086\n"), Some("Intel"));
        assert_eq!(parse_drm_vendor("0xdead"), None);
    }

    #[test]
    fn gpu_never_panics() {
        let ctx = crate::fetch::Context::collect(&[crate::fetch::Module::Gpu]);
        if let Some(v) = gpu(&ctx) {
            assert!(!v.trim().is_empty());
        }
    }
}
