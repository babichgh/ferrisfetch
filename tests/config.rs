//! Persistent configuration is exercised with child-only environment changes.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

struct Sandbox(PathBuf);

impl Sandbox {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrisfetch-config-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn default_path(&self) -> PathBuf {
        self.0.join("xdg/ferrisfetch/config.json")
    }

    fn home_path(&self) -> PathBuf {
        self.0.join("home/.config/ferrisfetch/config.json")
    }

    fn write(&self, path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ferrisfetch"));
        command
            .current_dir(&self.0)
            .env("HOME", self.0.join("home"))
            .env("XDG_CONFIG_HOME", self.0.join("xdg"))
            .env("NO_COLOR", "1");
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn success(output: Output) -> String {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

fn failure(output: Output, path: &Path) -> String {
    assert!(!output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(&path.display().to_string()), "{stderr}");
    stderr
}

fn keys(stdout: &str) -> Vec<&str> {
    stdout
        .lines()
        .map(|line| line.split_once(": ").unwrap().0)
        .collect()
}

#[test]
fn generates_default_xdg_config_and_it_can_be_loaded() {
    let sandbox = Sandbox::new();
    let path = sandbox.default_path();
    let stdout = success(sandbox.run(&["--gen-config"]));
    assert!(stdout.contains(&path.display().to_string()));
    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let modules = json["modules"].as_array().unwrap();
    assert!(modules.iter().any(|module| module == "os"));
    assert!(!modules.iter().any(|module| module == "cpu-usage"));
    assert_eq!(json["hide-modules"], serde_json::json!([]));
    assert_eq!(json["cpu-usage-interval"], 200);
    assert_eq!(json["no-color"], false);
    assert_eq!(json["nerd-separator"], false);
    assert!(!sandbox.home_path().exists());
    assert_eq!(
        keys(&success(sandbox.run(&["--pipe", "--modules", "os"]))),
        ["OS"]
    );
}

#[test]
fn generation_and_loading_fall_back_to_home() {
    for xdg in [None, Some(""), Some("relative")] {
        let sandbox = Sandbox::new();
        let mut command = sandbox.command();
        match xdg {
            Some(value) => command.env("XDG_CONFIG_HOME", value),
            None => command.env_remove("XDG_CONFIG_HOME"),
        };
        success(command.arg("--gen-config").output().unwrap());
        assert!(sandbox.home_path().is_file());
        assert!(!sandbox.default_path().exists());
        sandbox.write(&sandbox.home_path(), r#"{"modules":["ram","os"]}"#);
        let mut command = sandbox.command();
        match xdg {
            Some(value) => command.env("XDG_CONFIG_HOME", value),
            None => command.env_remove("XDG_CONFIG_HOME"),
        };
        let stdout = success(command.arg("--pipe").output().unwrap());
        assert_eq!(keys(&stdout), ["RAM", "OS"]);
    }
}

#[test]
fn custom_generation_never_overwrites_an_existing_file() {
    let sandbox = Sandbox::new();
    let path = sandbox.0.join("nested/custom.json");
    let args = ["--gen-config", "--config", path.to_str().unwrap()];
    success(sandbox.run(&args));
    let original = fs::read(&path).unwrap();
    failure(sandbox.run(&args), &path);
    assert_eq!(fs::read(&path).unwrap(), original);
    sandbox.write(&path, "not even valid JSON\n");
    failure(sandbox.run(&args), &path);
    assert_eq!(fs::read_to_string(&path).unwrap(), "not even valid JSON\n");
    assert!(!sandbox.default_path().exists());
}

#[test]
fn default_generation_never_overwrites_existing_config() {
    let sandbox = Sandbox::new();
    sandbox.write(&sandbox.default_path(), "existing contents");
    failure(sandbox.run(&["--gen-config"]), &sandbox.default_path());
    assert_eq!(
        fs::read_to_string(sandbox.default_path()).unwrap(),
        "existing contents"
    );
}

#[test]
fn default_config_preserves_order_deduplicates_and_hides_modules() {
    let sandbox = Sandbox::new();
    sandbox.write(
        &sandbox.default_path(),
        r#"{"modules":["ram","os","ram","kernel"],"hide-modules":["os"]}"#,
    );
    assert_eq!(keys(&success(sandbox.run(&["--pipe"]))), ["RAM", "Kernel"]);
    let json: serde_json::Value = serde_json::from_str(&success(sandbox.run(&["--json"]))).unwrap();
    assert_eq!(json.as_object().unwrap().len(), 2);
    assert!(json["ram"].is_string());
    assert!(json["kernel"].is_string());
}

#[test]
fn custom_config_replaces_default_config() {
    let sandbox = Sandbox::new();
    sandbox.write(&sandbox.default_path(), "malformed default");
    let path = sandbox.0.join("custom.json");
    sandbox.write(&path, r#"{"modules":["kernel","os","kernel"]}"#);
    let stdout = success(sandbox.run(&["--config", path.to_str().unwrap(), "--pipe"]));
    assert_eq!(keys(&stdout), ["Kernel", "OS"]);
}

#[test]
fn empty_modules_stays_empty_in_pipe_and_json() {
    let sandbox = Sandbox::new();
    sandbox.write(
        &sandbox.default_path(),
        r#"{"modules":[],"hide-modules":["os"]}"#,
    );
    assert_eq!(success(sandbox.run(&["--pipe"])), "");
    assert_eq!(success(sandbox.run(&["--json"])).trim(), "{}");
}

#[test]
fn cli_modules_and_hide_override_configured_selection() {
    let sandbox = Sandbox::new();
    sandbox.write(
        &sandbox.default_path(),
        r#"{"modules":["ram","os","kernel"],"hide-modules":["os"]}"#,
    );
    assert_eq!(
        keys(&success(sandbox.run(&["--pipe", "--hide-modules", "ram"]))),
        ["OS", "Kernel"]
    );
    assert_eq!(
        keys(&success(sandbox.run(&[
            "--pipe",
            "--modules",
            "os,os",
            "--hide-modules",
            "os"
        ]))),
        ["OS"]
    );
}

#[test]
fn configured_hide_also_filters_the_default_set() {
    let sandbox = Sandbox::new();
    sandbox.write(&sandbox.default_path(), r#"{"hide-modules":["os","cpu"]}"#);
    let stdout = success(sandbox.run(&["--pipe"]));
    let keys = keys(&stdout);
    assert!(!keys.contains(&"OS"));
    assert!(!keys.contains(&"CPU"));
    assert!(keys.contains(&"RAM"));
}

#[test]
fn logo_config_is_loaded_and_cli_can_override_it() {
    let sandbox = Sandbox::new();
    sandbox.write(
        &sandbox.default_path(),
        r#"{"modules":["os"],"logo":"off","no-color":true}"#,
    );
    let stdout = success(sandbox.run(&[]));
    assert!(
        stdout.lines().all(|line| line.starts_with("OS")),
        "{stdout}"
    );
    let stdout = success(sandbox.run(&["--logo", "arch"]));
    assert!(stdout.contains('`'), "Arch art missing: {stdout}");
    sandbox.write(
        &sandbox.default_path(),
        r#"{"modules":["os"],"logo":"arch"}"#,
    );
    let stdout = success(sandbox.run(&["--logo-off"]));
    assert!(
        stdout.lines().all(|line| line.starts_with("OS")),
        "{stdout}"
    );
}

#[test]
fn interval_and_separator_config_can_be_used_with_cli_overrides() {
    let sandbox = Sandbox::new();
    sandbox.write(
        &sandbox.default_path(),
        r#"{"modules":["cpu-usage"],"cpu-usage-interval":250,"nerd-separator":true}"#,
    );
    let stdout = success(sandbox.run(&["--pipe"]));
    assert!(stdout.starts_with("CPU Usage\u{f420} "), "{stdout}");
    let stdout = success(sandbox.run(&["--json", "--cpu-usage-interval", "200"]));
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(json["cpu_usage"].as_str().unwrap().ends_with('%'));
}

#[test]
fn no_config_bypasses_a_malformed_default() {
    let sandbox = Sandbox::new();
    sandbox.write(&sandbox.default_path(), "{");
    failure(
        sandbox.run(&["--pipe", "--modules", "os"]),
        &sandbox.default_path(),
    );
    assert_eq!(
        keys(&success(sandbox.run(&[
            "--no-config",
            "--pipe",
            "--modules",
            "os"
        ]))),
        ["OS"]
    );
    assert_eq!(fs::read_to_string(sandbox.default_path()).unwrap(), "{");
}

#[test]
fn malformed_unknown_and_wrongly_typed_settings_are_errors() {
    let sandbox = Sandbox::new();
    let path = sandbox.default_path();
    for text in [
        "{",
        r#"{"moduels":[]}"#,
        r#"{"modules":["not-a-module"]}"#,
        r#"{"hide-modules":["not-a-module"]}"#,
        r#"{"modules":"os"}"#,
        r#"{"modules":[1]}"#,
        r#"{"hide-modules":false}"#,
        r#"{"logo":42}"#,
        r#"{"no-color":"true"}"#,
        r#"{"nerd-separator":1}"#,
        r#"{"cpu-usage-interval":"200"}"#,
        r#"{"cpu-usage-interval":-1}"#,
        r#"{"cpu-usage-interval":200.5}"#,
        r#"{"cpu-usage-interval":0}"#,
        r#"{"cpu-usage-interval":199}"#,
    ] {
        sandbox.write(&path, text);
        // Explicit CLI settings must not silently mask invalid config fields.
        failure(
            sandbox.run(&["--pipe", "--modules", "os", "--cpu-usage-interval", "200"]),
            &path,
        );
    }
}

#[test]
fn missing_default_is_ok_but_missing_explicit_config_is_an_error() {
    let sandbox = Sandbox::new();
    assert_eq!(
        keys(&success(sandbox.run(&["--pipe", "--modules", "os"]))),
        ["OS"]
    );
    let path = sandbox.0.join("missing.json");
    failure(
        sandbox.run(&["--pipe", "--config", path.to_str().unwrap()]),
        &path,
    );
    assert!(!path.exists());
    assert!(!sandbox.default_path().exists());
    assert!(!sandbox.home_path().exists());
    assert!(!sandbox.0.join("xdg").exists());
    assert!(!sandbox.0.join("home").exists());
}

#[test]
fn informational_flags_do_not_load_a_malformed_config() {
    let sandbox = Sandbox::new();
    sandbox.write(&sandbox.default_path(), "{");
    for flag in ["--help", "--version", "--logo-list"] {
        assert!(!success(sandbox.run(&[flag])).is_empty());
    }
}

#[test]
fn contradictory_config_options_are_rejected() {
    let sandbox = Sandbox::new();
    for args in [
        vec!["--no-config", "--config", "custom.json"],
        vec!["--no-config", "--gen-config"],
        vec!["--gen-config", "--logo-list"],
    ] {
        let output = sandbox.run(&args);
        assert!(!output.status.success(), "{output:?}");
        assert!(!output.stderr.is_empty());
    }
    assert!(!sandbox.default_path().exists());
}
