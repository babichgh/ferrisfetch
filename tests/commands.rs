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
        let output = Command::new(env!("CARGO_BIN_EXE_ferrisfetch"))
            .args(["--pipe", "--modules", module])
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
        "printf '00:02.0 VGA compatible controller: Fixture GPU\\n'",
    );
    assert_eq!(tools.run("gpu"), "GPU: Fixture GPU\n");
}

#[test]
fn gpu_falls_back_after_failure_or_signal() {
    let tools = Tools::new();
    tools.add(
        "lspci",
        "printf '00:02.0 VGA compatible controller: Fixture GPU\\n'",
    );
    for script in ["printf partial; exit 1", "kill -TERM $$"] {
        tools.add("nvidia-smi", script);
        assert_eq!(tools.run("gpu"), "GPU: Fixture GPU\n");
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
[ "$1" = '--query-gpu=name,memory.total' ] || exit 1
[ "$2" = '--format=csv,noheader,nounits' ] || exit 1
printf 'Fixture NVIDIA, 1024\n'
"#,
    );
    assert_eq!(tools.run("gpu"), "GPU: Fixture NVIDIA (1.0 GiB)\n");
}
