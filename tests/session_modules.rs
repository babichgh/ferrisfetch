//! Controlled environment and parent shell: independent of the desktop running CI.

use std::process::{Command, Output};

#[cfg(target_os = "linux")]
fn run(args: &[&str]) -> Output {
    // Keep sh alive while ferrisfetch runs, rather than letting it exec the
    // final command. /proc then exposes a real, deterministic shell ancestor.
    Command::new("/bin/sh")
        .args(["-c", "\"$@\" & wait $!", "session-fixture"])
        .arg(env!("CARGO_BIN_EXE_ferrisfetch"))
        .args(args)
        .env_clear()
        .env("SHELL", "/bin/zsh")
        .env("TMUX", "/fixture/socket,123,0")
        .env("SSH_CONNECTION", "fixture")
        .env("XDG_CURRENT_DESKTOP", "ubuntu:GNOME")
        .output()
        .unwrap()
}

#[cfg(target_os = "linux")]
fn expected_shell() -> String {
    std::fs::canonicalize("/bin/sh")
        .unwrap()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string()
}

#[cfg(target_os = "linux")]
#[test]
fn selected_session_modules_pipe_order_and_missing_wm() {
    let out = run(&["--pipe", "--modules", "wm,de,terminal,shell,de"]);
    assert!(out.status.success(), "{:?}", out);
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("DE: GNOME\nTerminal: tmux\nShell: {}\n", expected_shell())
    );
}

#[cfg(target_os = "linux")]
#[test]
fn session_json_keeps_flat_string_contract_and_omits_missing() {
    let out = run(&["--json", "--modules", "shell,terminal,de,wm"]);
    assert!(out.status.success());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "shell": expected_shell(), "terminal": "tmux", "de": "GNOME"
        })
    );
}

#[test]
fn de_and_wm_are_separate_and_ordered() {
    for (flag, expected) in [
        ("--pipe", "WM: Hyprland\nDE: KDE Plasma\n"),
        ("--json", "{\"wm\":\"Hyprland\",\"de\":\"KDE Plasma\"}\n"),
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_ferrisfetch"))
            .args([flag, "--modules", "wm,de"])
            .env_clear()
            .env("HYPRLAND_INSTANCE_SIGNATURE", "fixture")
            .env("XDG_CURRENT_DESKTOP", "KDE")
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8(out.stdout).unwrap(), expected);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn every_session_module_is_individually_selectable() {
    for (module, prefix) in [
        ("shell", "Shell:"),
        ("terminal", "Terminal:"),
        ("de", "DE:"),
    ] {
        let out = run(&["--pipe", "--modules", module]);
        assert!(out.status.success());
        let text = String::from_utf8(out.stdout).unwrap();
        assert_eq!(text.lines().count(), 1);
        assert!(text.starts_with(prefix));
    }
}

#[test]
fn default_selection_excludes_session_modules_and_hide_accepts_them() {
    for hidden in ["gpu,monitor", "shell,terminal,de,wm,gpu,monitor"] {
        let out = Command::new(env!("CARGO_BIN_EXE_ferrisfetch"))
            .args(["--pipe", "--hide-modules", hidden])
            .env_clear()
            .env("HYPRLAND_INSTANCE_SIGNATURE", "fixture")
            .env("XDG_CURRENT_DESKTOP", "KDE")
            .env("TMUX", "fixture")
            .output()
            .unwrap();
        assert!(out.status.success());
        let text = String::from_utf8(out.stdout).unwrap();
        for label in ["Shell:", "Terminal:", "DE:", "WM:"] {
            assert!(!text.contains(label));
        }
    }
}
