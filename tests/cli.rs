//! End-to-end tests driving the compiled binary.

use std::process::Command;

fn run(args: &[&str]) -> (String, String, bool) {
    let out = Command::new(env!("CARGO_BIN_EXE_ferrisfetch"))
        .args(args)
        .output()
        .expect("ferrisfetch binary should be runnable");
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.success(),
    )
}

#[test]
fn version_uses_project_name() {
    let (stdout, stderr, ok) = run(&["--version"]);
    assert!(ok, "stderr: {stderr}");
    assert_eq!(
        stdout.trim(),
        concat!("ferrisfetch ", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn help_uses_project_name() {
    let (stdout, stderr, ok) = run(&["--help"]);
    assert!(ok, "stderr: {stderr}");
    assert!(stdout.contains("Usage: ferrisfetch"), "got {stdout}");
}

#[test]
fn default_run_succeeds() {
    let (stdout, stderr, ok) = run(&[]);
    assert!(ok, "stderr: {stderr}");
    assert!(!stdout.is_empty());
}

#[test]
fn pipe_output_has_no_ansi_escapes() {
    let (stdout, _, ok) = run(&["--pipe"]);
    assert!(ok);
    assert!(!stdout.contains('\x1b'), "pipe output must not be coloured");
}

#[test]
fn pipe_output_uses_key_colon_value() {
    let (stdout, _, _) = run(&["--pipe", "--modules", "os,ram"]);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "got {stdout}");
    assert!(lines[0].starts_with("OS: "), "got {}", lines[0]);
    assert!(lines[1].starts_with("RAM: "), "got {}", lines[1]);
}

#[test]
fn json_output_is_a_single_object() {
    let (stdout, _, ok) = run(&["--json", "--modules", "os"]);
    assert!(ok);
    let line = stdout.trim();
    assert!(line.starts_with('{') && line.ends_with('}'), "got {line}");
    assert!(line.contains("\"os\":"), "got {line}");
    assert!(!line.contains('\x1b'));
}

#[test]
fn hide_modules_removes_lines() {
    let (all, _, _) = run(&["--pipe"]);
    let (hidden, _, _) = run(&["--pipe", "--hide-modules", "gpu,disk,cpu"]);
    assert!(all.lines().count() > hidden.lines().count());
    assert!(!hidden.contains("GPU:"));
    assert!(!hidden.contains("CPU:"));
}

#[test]
fn modules_order_is_respected() {
    let (stdout, _, _) = run(&["--pipe", "--modules", "ram,os"]);
    let keys: Vec<&str> = stdout
        .lines()
        .map(|l| l.split(':').next().unwrap())
        .collect();
    assert_eq!(keys, vec!["RAM", "OS"]);
}

#[test]
fn logo_off_produces_no_art() {
    let (with_logo, _, _) = run(&["--logo", "off", "--modules", "os"]);
    for line in with_logo.lines() {
        assert!(line.starts_with("OS"), "art leaked into output: {line:?}");
    }
}

#[test]
fn explicit_logo_is_used() {
    let (stdout, _, ok) = run(&["--logo", "arch", "--modules", "os"]);
    assert!(ok);
    assert!(stdout.contains("OS"), "got {stdout}");
    // The Arch art's first line is a lone backtick.
    assert!(stdout.contains('`'), "arch art missing: {stdout}");
}

#[test]
fn no_color_flag_emits_plain_text() {
    let (stdout, _, _) = run(&["--no-color"]);
    assert!(!stdout.contains('\x1b'), "got {stdout:?}");
}

#[test]
fn piped_output_is_never_coloured() {
    // stdout is a pipe under test, so colours must be off regardless.
    let (stdout, _, _) = run(&[]);
    assert!(!stdout.contains('\x1b'));
}

#[test]
fn unknown_module_exits_with_error() {
    let (_, stderr, ok) = run(&["--modules", "nonexistent"]);
    assert!(!ok, "should exit non-zero");
    assert!(!stderr.is_empty());
}

#[test]
fn version_flag_prints_version() {
    let (stdout, _, ok) = run(&["--version"]);
    assert!(ok);
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn help_lists_every_module() {
    let (stdout, _, ok) = run(&["--help"]);
    assert!(ok);
    for m in [
        "os",
        "host",
        "kernel",
        "uptime",
        "cpu",
        "cpu-usage",
        "gpu",
        "ram",
        "disk",
        "monitor",
        "battery",
        "swap",
        "shell",
        "terminal",
        "de",
        "wm",
    ] {
        assert!(stdout.contains(m), "{m} missing from --help");
    }
}

#[test]
fn cpu_info_and_usage_are_separate_modules() {
    let (stdout, stderr, ok) = run(&["--pipe", "--modules", "cpu,cpu-usage"]);
    assert!(ok, "stderr: {stderr}");
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "got {stdout}");
    assert!(lines[0].starts_with("CPU: "), "got {stdout}");
    assert!(!lines[0].contains('%'), "got {stdout}");
    let usage = lines[1].strip_prefix("CPU Usage: ").unwrap();
    let percent: u32 = usage.strip_suffix('%').unwrap().parse().unwrap();
    assert!(percent <= 100, "got {stdout}");
}

#[test]
fn cpu_usage_json_uses_a_separate_key() {
    let (stdout, stderr, ok) = run(&[
        "--json",
        "--modules",
        "cpu-usage",
        "--cpu-usage-interval",
        "250",
    ]);
    assert!(ok, "stderr: {stderr}");
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json.as_object().unwrap().len(), 1);
    assert!(json["cpu_usage"].as_str().unwrap().ends_with('%'));
}

#[test]
fn default_output_does_not_include_cpu_usage() {
    let (stdout, stderr, ok) = run(&["--pipe"]);
    assert!(ok, "stderr: {stderr}");
    assert!(!stdout.contains("CPU Usage:"), "got {stdout}");
}

#[test]
fn cpu_usage_can_be_hidden_from_defaults_without_changing_them() {
    let (stdout, stderr, ok) = run(&["--pipe", "--hide-modules", "cpu-usage"]);
    assert!(ok, "stderr: {stderr}");
    assert!(stdout.lines().any(|line| line.starts_with("CPU: ")));
    assert!(!stdout.contains("CPU Usage:"));
}

#[test]
fn invalid_cpu_usage_interval_exits_with_error() {
    let (_, stderr, ok) = run(&["--modules", "cpu-usage", "--cpu-usage-interval", "199"]);
    assert!(!ok);
    assert!(!stderr.is_empty());
}

#[test]
fn monitor_module_is_selectable_even_on_headless_hosts() {
    let (_, stderr, ok) = run(&["--pipe", "--modules", "monitor"]);
    assert!(ok, "stderr: {stderr}");
}

#[test]
fn battery_and_swap_pipe_selection_and_order() {
    // Hardware-dependent entries may be absent; OS/RAM anchor the order.
    let (stdout, stderr, ok) = run(&["--pipe", "--modules", "os,battery,ram,swap"]);
    assert!(ok, "stderr: {stderr}");
    assert!(!stdout.contains('\x1b'));
    let keys: Vec<_> = stdout
        .lines()
        .map(|line| line.split_once(": ").unwrap().0)
        .collect();
    assert_eq!(keys[0], "OS");
    let ram = keys.iter().position(|key| *key == "RAM").unwrap();
    assert!(keys[1..ram].iter().all(|key| key.starts_with("Battery ")));
    assert!(keys[ram + 1..].iter().all(|key| *key == "Swap"));

    let (stdout, stderr, ok) = run(&["--pipe", "--modules", "swap,battery"]);
    assert!(ok, "stderr: {stderr}");
    let mut battery_seen = false;
    for line in stdout.lines() {
        if line.starts_with("Battery ") {
            battery_seen = true;
        } else {
            assert!(line.starts_with("Swap: "), "got {stdout}");
            assert!(!battery_seen, "got {stdout}");
        }
    }
}

#[test]
fn battery_and_swap_json_preserves_string_object_contract() {
    let (stdout, stderr, ok) = run(&["--json", "--modules", "battery,swap"]);
    assert!(ok, "stderr: {stderr}");
    assert!(!stdout.contains('\x1b'));
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    for (key, value) in json.as_object().unwrap() {
        assert!(key == "swap" || key.starts_with("battery_"));
        let value = value.as_str().unwrap();
        assert!(!value.is_empty());
        assert!(!value.contains("unknown"));
        if key == "swap" {
            assert!(value.contains(" / ") && value.ends_with("%)"));
        }
    }
}

#[test]
fn battery_and_swap_are_not_in_default_output() {
    let (stdout, stderr, ok) = run(&["--pipe"]);
    assert!(ok, "stderr: {stderr}");
    assert!(!stdout.contains("Battery "));
    assert!(!stdout.contains("Swap:"));
    let (stdout, stderr, ok) = run(&["--json", "--hide-modules", "battery,swap"]);
    assert!(ok, "stderr: {stderr}");
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(
        json.as_object()
            .unwrap()
            .keys()
            .all(|key| key != "swap" && !key.starts_with("battery_"))
    );
}

#[test]
fn duplicate_modules_are_rendered_once() {
    let (stdout, _, _) = run(&["--pipe", "--modules", "ram,ram,ram"]);
    assert_eq!(stdout.lines().count(), 1, "got {stdout}");
}

#[test]
fn nerd_separator_replaces_colon() {
    let (stdout, _, _) = run(&["--pipe", "--nerd-separator", "--modules", "os"]);
    assert!(!stdout.contains("OS: "), "got {stdout}");
    assert!(stdout.contains('\u{f420}'), "got {stdout:?}");
}

#[test]
fn logo_list_lists_every_logo() {
    let (stdout, _, ok) = run(&["--logo-list"]);
    assert!(ok);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(lines.len() > 500, "got {} logos", lines.len());
    assert!(
        lines.iter().any(|l| l.starts_with("fedora")),
        "fedora missing"
    );
}

#[test]
fn logo_list_has_no_duplicates() {
    let (stdout, _, _) = run(&["--logo-list"]);
    let mut names: Vec<&str> = stdout
        .lines()
        .map(|l| l.split('\t').next().unwrap())
        .collect();
    let total = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), total, "duplicate primary names in --logo-list");
}

#[test]
fn every_listed_logo_resolves() {
    let (stdout, _, _) = run(&["--logo-list"]);
    for line in stdout.lines() {
        let name = line.split('\t').next().unwrap();
        let (out, _, ok) = run(&["--no-color", "--logo", name, "--modules", "os"]);
        assert!(ok, "--logo {name} failed");
        assert!(out.contains("OS"), "--logo {name} produced no output");
    }
}

#[test]
fn aliases_resolve_to_their_logo() {
    let (stdout, _, _) = run(&["--logo-list"]);
    // A logo advertising aliases must accept at least one of them.
    let with_alias = stdout
        .lines()
        .find(|l| l.contains('\t'))
        .expect("some logo should declare aliases");
    let alias = with_alias
        .split('\t')
        .nth(1)
        .unwrap()
        .split(' ')
        .next()
        .unwrap();
    let (out, _, ok) = run(&["--no-color", "--logo", alias, "--modules", "os"]);
    assert!(ok, "alias {alias} failed");
    assert!(out.contains("OS"));
}

#[test]
fn unknown_logo_falls_back_instead_of_failing() {
    let (out, _, ok) = run(&["--no-color", "--logo", "no-such-logo", "--modules", "os"]);
    assert!(ok);
    assert!(out.contains("OS"), "got {out}");
}
