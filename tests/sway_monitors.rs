//! Monitor routing tests use only child-process environment and fake tools.
#![cfg(unix)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Command};

struct Tools(PathBuf);

impl Tools {
    fn new(sway: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrisfetch-sway-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let tools = Self(path);
        fs::write(
            tools.0.join("outputs.json"),
            include_str!("fixtures/sway-outputs.json"),
        )
        .unwrap();
        tools.add("swaymsg", sway);
        tools.add(
            "hyprctl",
            r#"printf '[{"name":"HYPR","width":1920,"height":1080}]'"#,
        );
        tools.add("xrandr", "printf 'X11 connected 1920x1080+0+0\n'");
        tools
    }

    fn add(&self, name: &str, script: &str) {
        let path = self.0.join(name);
        let script = format!(
            "#!/bin/sh\nprintf '{name}\\n' >> '{}'/calls\n{script}\n",
            self.0.display()
        );
        fs::write(&path, script).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }

    fn run(&self, vars: &[(&str, &str)], args: &[&str]) -> (String, String) {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ferrisfetch"));
        for key in [
            "XDG_SESSION_TYPE",
            "XDG_CURRENT_DESKTOP",
            "XDG_SESSION_DESKTOP",
            "SWAYSOCK",
            "HYPRLAND_INSTANCE_SIGNATURE",
            "WAYLAND_DISPLAY",
            "DISPLAY",
        ] {
            cmd.env_remove(key);
        }
        let _ = fs::remove_file(self.0.join("calls"));
        let output = cmd
            .args(args)
            .env("PATH", &self.0)
            .envs(vars.iter().copied())
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        (
            String::from_utf8(output.stdout).unwrap(),
            fs::read_to_string(self.0.join("calls")).unwrap_or_default(),
        )
    }
}

impl Drop for Tools {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const SWAY: &str = r#"
[ "$LC_ALL" = C ] && [ "$LANG" = C ] || exit 1
[ "$#" -eq 3 ] && [ "$1" = -t ] && [ "$2" = get_outputs ] && [ "$3" = -r ] || exit 1
exec /bin/cat "${0%/*}/outputs.json"
"#;

#[test]
fn sway_preserves_numbering_order_and_json_schema() {
    let tools = Tools::new(SWAY);
    let vars = [("SWAYSOCK", "/fixture"), ("DISPLAY", ":99")];
    let (text, calls) = tools.run(&vars, &["--pipe", "--modules", "monitor"]);
    assert_eq!(calls, "swaymsg\n");
    assert_eq!(
        text,
        "Monitor: DP-1: 3840x2160 (24.0\") @ 59.94 Hz\nMonitor 2: eDP-1: 2560x1600 @ 120.00 Hz\n"
    );
    let (json, _) = tools.run(&vars, &["--json", "--modules", "monitor"]);
    let json: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(json["monitor"], "DP-1: 3840x2160 (24.0\") @ 59.94 Hz");
    assert_eq!(json["monitor_2"], "eDP-1: 2560x1600 @ 120.00 Hz");
    assert_eq!(json.as_object().unwrap().len(), 2);
    let (_, calls) = tools.run(&vars, &["--pipe", "--modules", "os"]);
    assert!(calls.is_empty());
}

#[test]
fn selected_wayland_backend_never_falls_back() {
    let tools = Tools::new(SWAY);
    fs::remove_file(tools.0.join("swaymsg")).unwrap();
    let (text, calls) = tools.run(
        &[("SWAYSOCK", "/fixture"), ("DISPLAY", ":99")],
        &["--pipe", "--modules", "monitor"],
    );
    assert!(text.is_empty());
    assert!(calls.is_empty());
    for script in [
        "exit 1",
        "printf broken",
        "printf '[]'",
        "while :; do printf 'excessive stderr' >&2; done",
    ] {
        let tools = Tools::new(script);
        let (text, calls) = tools.run(
            &[("SWAYSOCK", "/fixture"), ("DISPLAY", ":99")],
            &["--pipe", "--modules", "monitor"],
        );
        assert!(text.is_empty());
        assert_eq!(calls, "swaymsg\n");
    }
    let tools = Tools::new(SWAY);
    tools.add("hyprctl", "while :; do printf 'excessive stderr' >&2; done");
    let (text, calls) = tools.run(
        &[
            ("HYPRLAND_INSTANCE_SIGNATURE", "fixture"),
            ("DISPLAY", ":99"),
        ],
        &["--pipe", "--modules", "monitor"],
    );
    assert!(text.is_empty());
    assert_eq!(calls, "hyprctl\n");
}

#[test]
fn only_the_selected_backend_is_executed() {
    let tools = Tools::new(SWAY);
    for (vars, expected) in [
        (
            vec![
                ("XDG_SESSION_TYPE", "x11"),
                ("DISPLAY", ":99"),
                ("SWAYSOCK", "/stale"),
            ],
            "xrandr\n",
        ),
        (
            vec![("XDG_CURRENT_DESKTOP", "Hyprland"), ("SWAYSOCK", "/stale")],
            "hyprctl\n",
        ),
        (
            vec![
                ("XDG_CURRENT_DESKTOP", "sway"),
                ("HYPRLAND_INSTANCE_SIGNATURE", "stale"),
            ],
            "swaymsg\n",
        ),
        (
            vec![("WAYLAND_DISPLAY", "wayland-1"), ("DISPLAY", ":99")],
            "",
        ),
        (
            vec![
                ("SWAYSOCK", "/sway"),
                ("HYPRLAND_INSTANCE_SIGNATURE", "hypr"),
            ],
            "",
        ),
        (vec![], ""),
    ] {
        let (_, calls) = tools.run(&vars, &["--pipe", "--modules", "monitor"]);
        assert_eq!(calls, expected, "{vars:?}");
    }
}
