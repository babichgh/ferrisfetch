//! Optional detector tools are replaced with controlled fixtures, independent
//! of installed drivers, PCI tools, or the active desktop session.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

struct Tools(PathBuf);

impl Tools {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrisfetch-command-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        // Do not reuse or remove a directory left by another process.
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn add(&self, name: &str, script: &str) {
        let path = self.0.join(name);
        fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn run(&self, module: &str) -> String {
        self.run_args(&["--pipe", "--modules", module])
    }

    fn run_args(&self, args: &[&str]) -> String {
        let output = Command::new(env!("CARGO_BIN_EXE_ferrisfetch"))
            .args(args)
            .env("PATH", &self.0)
            .env("LC_ALL", "ferrisfetch-test-invalid-locale")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        String::from_utf8(output.stdout).unwrap()
    }
}

impl Drop for Tools {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn gpu_falls_back_after_a_timeout() {
    let tools = Tools::new();
    tools.add("nvidia-smi", "exec /bin/sleep 30");
    tools.add(
        "lspci",
        "printf 'abcd:01:00.0 VGA compatible controller: Fixture GPU\\n'",
    );
    assert_gpu_contains(&tools.run("gpu"), "Fixture GPU");
}

#[test]
fn gpu_falls_back_after_failure_or_signal() {
    let tools = Tools::new();
    tools.add(
        "lspci",
        "printf 'abcd:01:00.0 VGA compatible controller: Fixture GPU\\n'",
    );
    for script in ["printf partial; exit 1", "kill -TERM $$"] {
        tools.add("nvidia-smi", script);
        assert_gpu_contains(&tools.run("gpu"), "Fixture GPU");
    }
}

#[test]
fn monitor_falls_back_after_excessive_stderr() {
    let tools = Tools::new();
    tools.add("hyprctl", "while :; do printf 'too much output' >&2; done");
    tools.add(
        "xrandr",
        "printf 'DP-1 connected 1920x1080+0+0 530mm x 300mm\\n   1920x1080 60.00*+\\n'",
    );
    assert_eq!(
        tools.run("monitor"),
        "Monitor: DP-1: 1920x1080 (24.0\") @ 60.00 Hz\n"
    );
}

#[test]
fn monitor_is_omitted_when_no_backend_is_available() {
    let tools = Tools::new();
    assert!(tools.run("monitor").is_empty());
}

#[test]
fn successful_gpu_tool_receives_arguments_and_fixed_locale() {
    let tools = Tools::new();
    tools.add(
        "nvidia-smi",
        r#"
[ "$LC_ALL" = C ] && [ "$LANG" = C ] || exit 1
[ "$#" -eq 2 ] || exit 1
[ "$1" = '--query-gpu=pci.bus_id,name,memory.total' ] || exit 1
[ "$2" = '--format=csv,noheader,nounits' ] || exit 1
printf '0000abcd:01:00.0, Fixture NVIDIA, 1024\n'
"#,
    );
    assert_gpu_contains(&tools.run("gpu"), "Fixture NVIDIA (1.0 GiB)");
}

// The CLI always sees real sysfs, which can contain additional adapters. Use
// fixture-only PCI domains and assert contracts without assuming host hardware.
fn assert_gpu_contains(output: &str, card: &str) {
    assert!(output.starts_with("GPU: "), "{output}");
    assert_eq!(output.lines().count(), 1, "{output}");
    assert_eq!(output.matches(card).count(), 1, "{output}");
}

#[test]
fn hybrid_gpu_enrichment_preserves_single_entry_and_json_string() {
    let tools = Tools::new();
    tools.add(
        "lspci",
        r#"
[ "$#" -eq 1 ] && [ "$1" = '-D' ] || exit 1
printf 'abcd:02:00.0 3D controller: Fixture NVIDIA\nabcd:01:00.0 VGA compatible controller: Fixture Intel\n'
"#,
    );
    tools.add(
        "nvidia-smi",
        "printf '0000ABCD:02:00.0, Enriched NVIDIA, 4096\\n'",
    );
    let pipe = tools.run_args(&["--pipe", "--modules", "kernel,gpu,uptime"]);
    let lines: Vec<_> = pipe.lines().collect();
    assert_eq!(lines.len(), 3, "{pipe}");
    assert!(lines[0].starts_with("Kernel: "));
    assert!(lines[2].starts_with("Uptime: "));
    assert_gpu_contains(lines[1], "Fixture Intel + Enriched NVIDIA (4.0 GiB)");
    assert!(!pipe.contains("Fixture NVIDIA"));

    let json: serde_json::Value =
        serde_json::from_str(&tools.run_args(&["--json", "--modules", "gpu"])).unwrap();
    assert_eq!(json.as_object().unwrap().len(), 1);
    assert_eq!(json["gpu"].as_str().unwrap(), &lines[1][5..]);
}

#[test]
fn gpu_commands_are_not_called_unless_gpu_is_selected() {
    let tools = Tools::new();
    let marker = tools.0.join("called");
    let script = format!("printf called > '{}'", marker.display());
    tools.add("lspci", &script);
    tools.add("nvidia-smi", &script);
    assert!(tools.run("kernel").starts_with("Kernel: "));
    assert!(!marker.exists());
    tools.run_args(&["--pipe", "--hide-modules", "gpu"]);
    assert!(!marker.exists());
    tools.run("gpu");
    assert!(marker.exists());
}

#[test]
fn lspci_failure_does_not_discard_nvidia_data() {
    let tools = Tools::new();
    tools.add("lspci", "printf 'partial output'; exit 1");
    tools.add(
        "nvidia-smi",
        "printf '0000abcd:01:00.0, Fixture NVIDIA, [N/A]\\n'",
    );
    assert_gpu_contains(&tools.run("gpu"), "Fixture NVIDIA");
}
