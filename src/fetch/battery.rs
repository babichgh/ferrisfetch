//! Optional Linux power-supply batteries. No commands or privileged access.

use std::fs;
use std::path::Path;

use crate::render::Entry;

#[derive(Debug, PartialEq, Eq)]
struct Battery {
    id: String,
    capacity: Option<u8>,
    status: Option<&'static str>,
}

fn parse(id: String, capacity: Option<&str>, status: Option<&str>) -> Battery {
    Battery {
        id,
        capacity: capacity
            .and_then(|s| s.trim().parse::<u8>().ok())
            .filter(|n| *n <= 100),
        status: match status.map(str::trim) {
            Some("Charging") => Some("charging"),
            Some("Discharging") => Some("discharging"),
            Some("Full") => Some("full"),
            _ => None,
        },
    }
}

fn format(battery: Battery) -> Option<Entry> {
    let value = match (battery.capacity, battery.status) {
        (Some(percent), Some(status)) => format!("{percent}% ({status})"),
        (Some(percent), None) => format!("{percent}%"),
        (None, Some(status)) => status.to_string(),
        (None, None) => return None,
    };
    Some(Entry {
        key: format!("Battery {}", battery.id),
        value,
    })
}

fn read_batteries(root: &Path) -> Vec<Battery> {
    let Ok(devices) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut batteries = Vec::new();
    for device in devices.flatten() {
        let path = device.path();
        if fs::read_to_string(path.join("type"))
            .ok()
            .as_deref()
            .map(str::trim)
            != Some("Battery")
        {
            continue;
        }
        // Absent removable batteries can retain a sysfs directory.
        if fs::read_to_string(path.join("present"))
            .ok()
            .as_deref()
            .map(str::trim)
            == Some("0")
        {
            continue;
        }
        let Some(id) = device.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let capacity = fs::read_to_string(path.join("capacity")).ok();
        let status = fs::read_to_string(path.join("status")).ok();
        batteries.push(parse(id, capacity.as_deref(), status.as_deref()));
    }
    batteries.sort_by(|a, b| a.id.cmp(&b.id));
    batteries
}

fn entries_from(root: &Path) -> Vec<Entry> {
    read_batteries(root)
        .into_iter()
        .filter_map(format)
        .collect()
}

pub fn entries() -> Vec<Entry> {
    if cfg!(target_os = "linux") {
        entries_from(Path::new("/sys/class/power_supply"))
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "ferrisfetch-battery-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self(root)
        }

        fn device(&self, id: &str, files: &[(&str, &str)]) {
            let path = self.0.join(id);
            fs::create_dir(&path).unwrap();
            for (name, contents) in files {
                fs::write(path.join(name), contents).unwrap();
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn detects_by_type_and_formats_one_battery() {
        let f = Fixture::new();
        f.device(
            "pack",
            &[
                ("type", "Battery\n"),
                ("capacity", "42\n"),
                ("status", "Charging\n"),
            ],
        );
        f.device("BAT0", &[("type", "Mains"), ("capacity", "100")]);
        let entries = entries_from(&f.0);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, "Battery pack");
        assert_eq!(entries[0].value, "42% (charging)");
    }

    #[test]
    fn multiple_batteries_have_sorted_stable_ids() {
        let f = Fixture::new();
        f.device(
            "BAT2",
            &[("type", "Battery"), ("capacity", "100"), ("status", "Full")],
        );
        f.device(
            "BAT1",
            &[
                ("type", "Battery"),
                ("capacity", "0"),
                ("status", "Discharging"),
            ],
        );
        let entries = entries_from(&f.0);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key, "Battery BAT1");
        assert_eq!(entries[0].value, "0% (discharging)");
        assert_eq!(entries[1].key, "Battery BAT2");
        assert_eq!(entries[1].value, "100% (full)");
        fs::remove_dir_all(f.0.join("BAT1")).unwrap();
        assert_eq!(entries_from(&f.0)[0].key, "Battery BAT2");
    }

    #[test]
    fn missing_or_unreadable_data_is_omitted() {
        let f = Fixture::new();
        assert!(entries_from(&f.0).is_empty());
        assert!(entries_from(&f.0.join("missing")).is_empty());
        f.device("missing-type", &[("capacity", "50")]);
        f.device("empty", &[("type", "Battery")]);
        f.device(
            "absent",
            &[("type", "Battery"), ("present", "0"), ("capacity", "50")],
        );
        f.device(
            "bad",
            &[
                ("type", "Battery"),
                ("capacity", "101"),
                ("status", "Unknown"),
            ],
        );
        f.device("unreadable", &[("type", "Battery")]);
        // Directories fail read_to_string even when tests run as root.
        fs::create_dir(f.0.join("unreadable/capacity")).unwrap();
        fs::create_dir(f.0.join("unreadable/status")).unwrap();
        assert!(entries_from(&f.0).is_empty());
        assert!(entries_from(&f.0.join("bad/type")).is_empty());
    }

    #[test]
    fn partial_data_and_invalid_fields_are_independent() {
        for bad in [
            None,
            Some(""),
            Some("-1"),
            Some("101"),
            Some("256"),
            Some("NaN"),
            Some("1.5"),
        ] {
            let battery = parse("BAT0".into(), bad, Some("Full"));
            assert_eq!(format(battery).unwrap().value, "full");
        }
        for status in [None, Some("Unknown"), Some("Not charging"), Some("garbage")] {
            assert_eq!(
                format(parse("BAT0".into(), Some("50"), status))
                    .unwrap()
                    .value,
                "50%"
            );
        }
        let f = Fixture::new();
        f.device("percent", &[("type", "Battery"), ("capacity", "25")]);
        f.device("status", &[("type", "Battery"), ("status", "Discharging")]);
        let entries = entries_from(&f.0);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].value, "25%");
        assert_eq!(entries[1].value, "discharging");
    }
}
