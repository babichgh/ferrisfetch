//! KWin command fixtures; no real D-Bus service or desktop session is used.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "ferrisfetch-session-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        Self(dir)
    }

    fn tool(&self, name: &str, script: &str) {
        let path = self.0.join(name);
        fs::write(
            &path,
            format!("#!/bin/sh\nprintf '%s\\n' '{name}' >> \"$TRACE\"\n{script}\n"),
        )
        .unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn run(&self, modules: &str, vars: &[(&str, &str)]) -> String {
        let out = Command::new(env!("CARGO_BIN_EXE_ferrisfetch"))
            .args(["--pipe", "--modules", modules])
            .env_clear()
            .env("PATH", &self.0)
            .env("TRACE", self.0.join("trace"))
            .envs(vars.iter().copied())
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        assert!(out.stderr.is_empty(), "{out:?}");
        String::from_utf8(out.stdout).unwrap()
    }

    fn trace(&self) -> String {
        fs::read_to_string(self.0.join("trace")).unwrap_or_default()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const KDE: &[(&str, &str)] = &[("XDG_CURRENT_DESKTOP", "KDE"), ("DISPLAY", ":0")];

#[test]
fn kwin_query_checks_arguments_locale_and_response() {
    let fixture = Fixture::new();
    fixture.tool(
        "qdbus6",
        r#"
[ "$LC_ALL" = C ] && [ "$LANG" = C ] || exit 1
[ "$#" -eq 3 ] || exit 1
[ "$1" = org.kde.KWin ] && [ "$2" = /KWin ] || exit 1
[ "$3" = org.kde.KWin.supportInformation ] || exit 1
printf 'KWin Support Information:\nName: KWin\n'
"#,
    );
    assert_eq!(fixture.run("wm", KDE), "WM: KWin\n");
    assert_eq!(fixture.trace(), "qdbus6\n");
}

#[test]
fn kde5_uses_only_its_matching_tool() {
    let fixture = Fixture::new();
    fixture.tool("qdbus", "printf 'Name: KWin\\n'");
    fixture.tool("qdbus6", "exit 1");
    let mut vars = KDE.to_vec();
    vars.push(("KDE_SESSION_VERSION", "5"));
    assert_eq!(fixture.run("wm", &vars), "WM: KWin\n");
    assert_eq!(fixture.trace(), "qdbus\n");
}

#[test]
fn missing_failed_or_malformed_kwin_is_omitted_without_fanout() {
    let fixture = Fixture::new();
    assert_eq!(fixture.run("wm", KDE), "");
    for script in [
        "printf 'Name: KWin\\n'; exit 1",
        "printf 'Name: Mutter\\n'",
        "exit 0",
    ] {
        fixture.tool("qdbus6", script);
        assert_eq!(fixture.run("wm", KDE), "");
    }
    assert_eq!(fixture.trace(), "qdbus6\nqdbus6\nqdbus6\n");
}

#[test]
fn kwin_probe_is_bounded() {
    let fixture = Fixture::new();
    fixture.tool("qdbus6", "exec /bin/sleep 30");
    assert_eq!(fixture.run("wm", KDE), "");
    assert_eq!(fixture.trace(), "qdbus6\n");
}

#[test]
fn unrelated_modules_and_sessions_never_probe_compositor_tools() {
    let fixture = Fixture::new();
    for tool in ["qdbus6", "qdbus", "hyprctl", "swaymsg"] {
        fixture.tool(tool, "exit 1");
    }
    assert_eq!(fixture.run("de", KDE), "DE: KDE Plasma\n");
    fixture.run("shell,terminal", KDE);
    assert_eq!(
        fixture.run("wm", &[("XDG_CURRENT_DESKTOP", "GNOME"), ("DISPLAY", ":0")]),
        ""
    );
    let mut vars = KDE.to_vec();
    vars.push(("SSH_CONNECTION", "fixture"));
    assert_eq!(fixture.run("wm", &vars), "");
    vars.pop();
    vars.push(("SWAYSOCK", "/fixture/socket"));
    assert_eq!(fixture.run("wm", &vars), "WM: Sway\n");
    assert!(fixture.trace().is_empty());
}
