//! PCI GPU enumeration with optional lspci names and NVIDIA enrichment.
//! Device identity and ordering use PCI addresses, never model/vendor names.

use std::collections::BTreeMap;
use std::path::Path;

use crate::fetch::Context;
use crate::fetch::util::bytes;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct PciAddress {
    domain: u32,
    bus: u8,
    device: u8,
    function: u8,
}

impl PciAddress {
    /// Accept lspci's domain-less addresses, sysfs BDFs, and NVIDIA's
    /// eight-digit domains. Compare numeric components, not their spelling.
    fn parse(value: &str) -> Option<Self> {
        fn hex(value: &str, max_len: usize) -> Option<u32> {
            if value.is_empty()
                || value.len() > max_len
                || !value.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return None;
            }
            u32::from_str_radix(value, 16).ok()
        }
        let (prefix, function) = value.trim().rsplit_once('.')?;
        let mut parts = prefix.rsplit(':');
        let device = hex(parts.next()?, 2)?;
        let bus = hex(parts.next()?, 2)?;
        let domain = parts.next().map(|v| hex(v, 8)).unwrap_or(Some(0))?;
        let function = hex(function, 1)?;
        if parts.next().is_some() || device > 31 || function > 7 {
            return None;
        }
        Some(Self {
            domain,
            bus: bus as u8,
            device: device as u8,
            function: function as u8,
        })
    }
}

#[derive(Debug)]
struct Device {
    address: PciAddress,
    name: Option<String>,
    memory_bytes: Option<u64>,
}

pub fn gpu(_ctx: &Context) -> Option<String> {
    format_devices(&detect(
        Path::new("/sys/bus/pci/devices"),
        super::command::output,
    ))
}

/// Paths and command execution are injected only internally, so tests never
/// need the host's hardware, drivers, environment overrides, or privileged I/O.
fn detect(sysfs: &Path, mut output: impl FnMut(&str, &[&str]) -> Option<String>) -> Vec<Device> {
    let mut devices = BTreeMap::new();
    merge(&mut devices, read_sysfs(sysfs));
    if let Some(stdout) = output("lspci", &["-D"]) {
        merge(&mut devices, parse_lspci(&stdout));
    }
    if let Some(stdout) = output(
        "nvidia-smi",
        &[
            "--query-gpu=pci.bus_id,name,memory.total",
            "--format=csv,noheader,nounits",
        ],
    ) {
        merge(&mut devices, parse_nvidia_csv(&stdout));
    }
    devices.into_values().collect()
}

/// Sources run from least to most descriptive. Missing enrichment must not
/// erase a name or memory already supplied by another source.
fn merge(devices: &mut BTreeMap<PciAddress, Device>, incoming: Vec<Device>) {
    for device in incoming {
        devices
            .entry(device.address)
            .and_modify(|current| {
                if device.name.is_some() {
                    current.name.clone_from(&device.name);
                }
                if device.memory_bytes.is_some() {
                    current.memory_bytes = device.memory_bytes;
                }
            })
            .or_insert(device);
    }
}

fn format_devices(devices: &[Device]) -> Option<String> {
    let cards: Vec<_> = devices
        .iter()
        .map(|device| {
            let name = device.name.as_deref().unwrap_or("PCI GPU");
            match device.memory_bytes {
                Some(memory) => format!("{name} ({})", bytes(memory)),
                None => name.to_string(),
            }
        })
        .collect();
    match cards.len() {
        0 => None,
        1 => cards.into_iter().next(),
        n => Some(format!("{n}x {}", cards.join(" + "))),
    }
}

fn parse_hex(value: &str) -> Option<u32> {
    let value = value.trim();
    u32::from_str_radix(value.strip_prefix("0x").unwrap_or(value), 16).ok()
}

fn vendor_name(vendor: u32) -> Option<&'static str> {
    match vendor {
        0x10de => Some("NVIDIA"),
        0x1002 | 0x1022 => Some("AMD"),
        0x8086 => Some("Intel"),
        0x102b => Some("Matrox"),
        0x1a03 => Some("ASPEED"),
        0x1af4 => Some("Virtio"),
        _ => None,
    }
}

fn read_sysfs(root: &Path) -> Vec<Device> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let address = PciAddress::parse(&entry.file_name().to_string_lossy())?;
            let read_hex = |file| {
                std::fs::read_to_string(entry.path().join(file))
                    .ok()
                    .and_then(|value| parse_hex(&value))
            };
            // All PCI display subclasses, including VGA, 3D, and other display
            // controllers. No DRM node or loaded kernel driver is required.
            if read_hex("class")? >> 16 != 0x03 {
                return None;
            }
            let vendor = read_hex("vendor");
            let name = match vendor.and_then(vendor_name) {
                Some(name) => format!("{name} GPU"),
                None => match (vendor, read_hex("device")) {
                    (Some(vendor), Some(device)) => {
                        format!("PCI GPU [{vendor:04x}:{device:04x}]")
                    }
                    _ => "PCI GPU".to_string(),
                },
            };
            Some(Device {
                address,
                name: Some(name),
                memory_bytes: None,
            })
        })
        .collect()
}

fn parse_lspci(stdout: &str) -> Vec<Device> {
    stdout
        .lines()
        .filter_map(|line| {
            let (address, rest) = line.split_once(char::is_whitespace)?;
            let (class, name) = rest.trim().split_once(": ")?;
            if ![
                "VGA compatible controller",
                "3D controller",
                "Display controller",
            ]
            .iter()
            .any(|display| class.eq_ignore_ascii_case(display))
                || name.trim().is_empty()
            {
                return None;
            }
            Some(Device {
                address: PciAddress::parse(address)?,
                name: Some(name.trim().to_string()),
                memory_bytes: None,
            })
        })
        .collect()
}

fn parse_nvidia_csv(stdout: &str) -> Vec<Device> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.split(',').map(str::trim);
            let address = PciAddress::parse(parts.next()?)?;
            let name = parts.next().filter(|name| {
                !name.is_empty()
                    && !name.eq_ignore_ascii_case("N/A")
                    && !(name.starts_with('[') && name.ends_with(']'))
            });
            let memory_bytes = parts
                .next()
                .and_then(|memory| memory.parse::<u64>().ok())
                .and_then(|mib| mib.checked_mul(1024 * 1024));
            Some(Device {
                address,
                name: name.map(str::to_string),
                memory_bytes,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    const LSPCI: &str = "01:00.0 3D controller: NVIDIA Corporation GA102 [GeForce RTX 3090]\n\
        0000:00:02.0 VGA compatible controller: Intel Corporation UHD Graphics 770\n\
        00:00.0 Host bridge: Intel Corporation Host Bridge\n";
    const NVIDIA: &str = "00000000:01:00.0, NVIDIA GeForce RTX 3090, 24576\n";

    struct Sysfs(PathBuf);

    impl Sysfs {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "ferrisfetch-gpu-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn device(&self, address: &str, class: &str, vendor: &str) {
            let path = self.0.join(address);
            fs::create_dir(&path).unwrap();
            fs::write(path.join("class"), class).unwrap();
            fs::write(path.join("vendor"), vendor).unwrap();
            fs::write(path.join("device"), "0x1234\n").unwrap();
        }

        fn detect(&self, lspci: Option<&str>, nvidia: Option<&str>) -> Vec<Device> {
            detect(&self.0, |command, args| match command {
                "lspci" => {
                    assert_eq!(args, ["-D"]);
                    lspci.map(str::to_string)
                }
                "nvidia-smi" => {
                    assert_eq!(
                        args,
                        [
                            "--query-gpu=pci.bus_id,name,memory.total",
                            "--format=csv,noheader,nounits"
                        ]
                    );
                    nvidia.map(str::to_string)
                }
                other => panic!("unexpected command: {other}"),
            })
        }
    }

    impl Drop for Sysfs {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn intel_and_nvidia_are_merged_by_address_in_pci_order() {
        let sysfs = Sysfs::new();
        // Deliberately create devices in reverse PCI order.
        sysfs.device("0000:01:00.0", "0x030200\n", "0x10de\n");
        sysfs.device("0000:00:02.0", "0x030000\n", "0x8086\n");
        let devices = sysfs.detect(Some(LSPCI), Some(NVIDIA));
        assert_eq!(devices.len(), 2);
        assert_eq!(
            format_devices(&devices).as_deref(),
            Some("2x Intel Corporation UHD Graphics 770 + NVIDIA GeForce RTX 3090 (24.0 GiB)")
        );
    }

    #[test]
    fn amd_survives_successful_nvidia_query_without_lspci() {
        let sysfs = Sysfs::new();
        sysfs.device("0000:00:02.0", "0x038000", "0x1002");
        sysfs.device("0000:01:00.0", "0x030000", "0x10de");
        assert_eq!(
            format_devices(&sysfs.detect(None, Some(NVIDIA))).as_deref(),
            Some("2x AMD GPU + NVIDIA GeForce RTX 3090 (24.0 GiB)")
        );
    }

    #[test]
    fn identical_cards_at_distinct_addresses_are_not_collapsed() {
        let sysfs = Sysfs::new();
        sysfs.device("0000:02:00.0", "0x030000", "0x10de");
        sysfs.device("0000:01:00.0", "0x030000", "0x10de");
        let csv = "00000000:02:00.0, NVIDIA RTX 3060, 12288\n\
            00000000:01:00.0, NVIDIA RTX 3060, 12288\n";
        let lspci = "02:00.0 VGA compatible controller: NVIDIA RTX 3060\n\
            01:00.0 VGA compatible controller: NVIDIA RTX 3060\n";
        assert_eq!(
            format_devices(&sysfs.detect(Some(lspci), Some(csv))).as_deref(),
            Some("2x NVIDIA RTX 3060 (12.0 GiB) + NVIDIA RTX 3060 (12.0 GiB)")
        );
        assert_eq!(
            format_devices(&sysfs.detect(None, None)).as_deref(),
            Some("2x NVIDIA GPU + NVIDIA GPU")
        );
    }

    #[test]
    fn one_device_from_all_sources_produces_one_card() {
        let sysfs = Sysfs::new();
        sysfs.device("0000:01:00.0", "0x030000", "0x10de");
        let lspci = "01:00.0 VGA compatible controller: NVIDIA GA102 [GeForce RTX 3090]\n";
        let devices = sysfs.detect(Some(lspci), Some(NVIDIA));
        assert_eq!(devices.len(), 1);
        assert_eq!(
            format_devices(&devices).as_deref(),
            Some("NVIDIA GeForce RTX 3090 (24.0 GiB)")
        );
    }

    #[test]
    fn normalizes_pci_domains_case_and_padding() {
        let address = PciAddress::parse("0000:0a:1f.7").unwrap();
        for value in ["a:1F.7", "00000000:0A:1f.7", " 0000:0a:1F.7 "] {
            assert_eq!(PciAddress::parse(value), Some(address));
        }
        assert_eq!(
            PciAddress::parse("00000001:0A:1f.7"),
            PciAddress::parse("0001:0a:1f.7")
        );
        assert_ne!(PciAddress::parse("0001:0a:1f.7"), Some(address));
        for invalid in [
            "",
            "[N/A]",
            "01:00",
            "01:00.8",
            "01:20.0",
            "100:00.0",
            "-1:00.0",
            "0x00:01:00.0",
            "0:0:01:00.0",
            "000000000:01:00.0",
        ] {
            assert_eq!(PciAddress::parse(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn partial_nvidia_data_preserves_names_and_other_cards() {
        let sysfs = Sysfs::new();
        let lspci = "01:00.0 VGA compatible controller: NVIDIA Model A\n\
            02:00.0 VGA compatible controller: NVIDIA Model B\n\
            03:00.0 VGA compatible controller: AMD Model C\n";
        let nvidia = "00000000:01:00.0, [N/A], 4096\n\
            00000000:02:00.0, NVIDIA Model B, [Not Supported]\n\
            [N/A], inaccessible card, 1024\n";
        assert_eq!(
            format_devices(&sysfs.detect(Some(lspci), Some(nvidia))).as_deref(),
            Some("3x NVIDIA Model A (4.0 GiB) + NVIDIA Model B + AMD Model C")
        );
    }

    #[test]
    fn missing_and_overflowing_memory_are_omitted() {
        for memory in ["", "[N/A]", "N/A", "bad", "18446744073709551615"] {
            let cards = parse_nvidia_csv(&format!("01:00.0, NVIDIA GPU, {memory}\n"));
            assert_eq!(format_devices(&cards).as_deref(), Some("NVIDIA GPU"));
        }
        assert_eq!(
            format_devices(&parse_nvidia_csv("01:00.0, NVIDIA GPU\n")).as_deref(),
            Some("NVIDIA GPU")
        );
        assert_eq!(
            format_devices(&parse_nvidia_csv("01:00.0, [N/A], 2048\n")).as_deref(),
            Some("PCI GPU (2.0 GiB)")
        );
    }

    #[test]
    fn unreadable_sysfs_falls_back_to_either_command() {
        let sysfs = Sysfs::new();
        let unavailable = sysfs.0.join("not-a-directory");
        fs::write(&unavailable, "inaccessible sysfs fixture").unwrap();
        for path in [unavailable, sysfs.0.join("missing")] {
            for (lspci, nvidia, count) in [
                (Some(LSPCI), Some(NVIDIA), 2),
                (Some(LSPCI), None, 2),
                (None, Some(NVIDIA), 1),
                (None, None, 0),
            ] {
                let devices = detect(&path, |command, _| {
                    if command == "lspci" { lspci } else { nvidia }.map(str::to_string)
                });
                assert_eq!(devices.len(), count);
                assert_eq!(format_devices(&devices).is_none(), count == 0);
            }
        }
    }

    #[test]
    fn sysfs_survives_failed_commands_and_missing_attributes() {
        let sysfs = Sysfs::new();
        sysfs.device("0000:02:00.0", "0x030000", "0x8086");
        sysfs.device("0000:01:00.0", "0x030000", "0xdead");
        sysfs.device("0000:03:00.0", "0x030000", "malformed");
        sysfs.device("0000:04:00.0", "0x020000", "0x10de");
        sysfs.device("0000:05:00.0", "malformed", "0x10de");
        sysfs.device("not-a-pci-address", "0x030000", "0x10de");
        fs::remove_file(sysfs.0.join("0000:03:00.0/vendor")).unwrap();
        assert_eq!(
            format_devices(&sysfs.detect(None, None)).as_deref(),
            Some("3x PCI GPU [dead:1234] + Intel GPU + PCI GPU")
        );
    }

    #[test]
    fn deterministic_numeric_order_includes_domain_and_function() {
        let sysfs = Sysfs::new();
        let lspci = "0001:00:00.0 VGA compatible controller: Domain 1\n\
            0000:0a:00.1 Display controller: Function 1\n\
            0000:02:00.0 VGA compatible controller: Bus 2\n\
            0000:0a:00.0 3D controller: Function 0\n";
        assert_eq!(
            format_devices(&sysfs.detect(Some(lspci), None)).as_deref(),
            Some("4x Bus 2 + Function 0 + Function 1 + Domain 1")
        );
    }

    #[test]
    fn malformed_command_output_is_ignored() {
        let sysfs = Sysfs::new();
        assert!(
            sysfs
                .detect(
                    Some(
                        "garbage\n00:00.0 Host bridge: Other\n\
            bad VGA compatible controller: No address\n\
            00:02.0 VGA compatible controller: \n"
                    ),
                    Some("\n[N/A], 100\nbroken\n")
                )
                .is_empty()
        );
    }
}
