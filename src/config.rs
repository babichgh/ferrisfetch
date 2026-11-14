//! Persistent settings. CLI selection always takes precedence over the file.

use std::path::{Path, PathBuf};

use clap::ValueEnum;
use serde::Deserialize;

use crate::{cli::Cli, fetch::Module};

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
struct Config {
    modules: Option<Vec<String>>,
    hide_modules: Option<Vec<String>>,
    cpu_usage_interval: Option<u64>,
    logo: Option<String>,
    no_color: bool,
    nerd_separator: bool,
}

fn default_path() -> Result<PathBuf, String> {
    default_path_from(
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
    )
}

fn default_path_from(
    xdg: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Result<PathBuf, String> {
    let base = xdg
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            home.filter(|p| !p.is_empty())
                .map(|p| PathBuf::from(p).join(".config"))
        });
    base.map(|p| p.join("ferrisfetch/config.json"))
        .ok_or_else(|| "cannot locate config directory; use --config PATH".to_owned())
}

fn parse_modules(names: Option<Vec<String>>) -> Result<Option<Vec<Module>>, String> {
    names
        .map(|names| {
            names
                .iter()
                .map(|name| Module::from_str(name, false))
                .collect()
        })
        .transpose()
}

impl Config {
    fn apply(self, cli: &mut Cli) -> Result<(), String> {
        // Validate even overridden settings so mistakes never go unnoticed.
        let modules = parse_modules(self.modules)?;
        let hide = parse_modules(self.hide_modules)?;
        if self.cpu_usage_interval.is_some_and(|ms| ms < 200) {
            return Err("cpu-usage-interval must be at least 200 ms".to_owned());
        }
        if cli.modules.is_none() {
            cli.modules = modules;
            if let Some(hide) = cli.hide_modules.as_ref().or(hide.as_ref()) {
                let selected = cli
                    .modules
                    .get_or_insert_with(|| crate::fetch::DEFAULT_ORDER.to_vec());
                selected.retain(|m| !hide.contains(m));
            }
        }
        cli.cpu_usage_interval = cli.cpu_usage_interval.or(self.cpu_usage_interval);
        if cli.logo.is_none() {
            cli.logo = self.logo;
        }
        cli.no_color |= self.no_color;
        cli.nerd_separator |= self.nerd_separator;
        Ok(())
    }
}

pub fn load(cli: &mut Cli) -> Result<(), String> {
    if cli.no_config {
        return Ok(());
    }
    let explicit = cli.config.is_some();
    let path = match cli.config.clone().map(Ok).unwrap_or_else(default_path) {
        Ok(path) => path,
        Err(_) if !explicit => return Ok(()),
        Err(error) => return Err(error),
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if !explicit && error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let config: Config =
        serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    config
        .apply(cli)
        .map_err(|error| format!("{}: {error}", path.display()))
}

pub fn generate(path: Option<&Path>) -> Result<PathBuf, String> {
    let path = path
        .map(Path::to_path_buf)
        .map(Ok)
        .unwrap_or_else(default_path)?;
    let modules: Vec<_> = crate::fetch::DEFAULT_ORDER
        .iter()
        .map(|m| m.to_possible_value().unwrap().get_name().to_owned())
        .collect();
    let contents = serde_json::to_string_pretty(&serde_json::json!({
        "modules": modules,
        "hide-modules": [],
        "logo": null,
        "cpu-usage-interval": 200,
        "no-color": false,
        "nerd-separator": false
    }))
    .expect("default config is serializable")
        + "\n";
    let write = || -> std::io::Result<()> {
        use std::io::Write;
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.write_all(contents.as_bytes())
    };
    write().map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn xdg_path_and_home_fallback() {
        assert_eq!(
            default_path_from(Some("/xdg".into()), Some("/home/test".into())).unwrap(),
            PathBuf::from("/xdg/ferrisfetch/config.json")
        );
        for xdg in [None, Some("".into()), Some("relative".into())] {
            assert_eq!(
                default_path_from(xdg, Some("/home/test".into())).unwrap(),
                PathBuf::from("/home/test/.config/ferrisfetch/config.json")
            );
        }
        assert!(default_path_from(None, None).is_err());
    }

    #[test]
    fn config_validation_is_strict() {
        for text in [
            r#"{"modules":["nonsense"]}"#,
            r#"{"cpu-usage-interval":199}"#,
        ] {
            let config: Config = serde_json::from_str(text).unwrap();
            assert!(config.apply(&mut Cli::parse_from(["ferrisfetch"])).is_err());
        }
        for text in [r#"{"moduels":[]}"#, r#"{"modules":"os"}"#, "{"] {
            assert!(serde_json::from_str::<Config>(text).is_err());
        }
    }

    #[test]
    fn cli_overrides_config_and_hide_filters_configured_order() {
        for (args, expected) in [
            (vec![], vec![Module::Ram]),
            (vec!["--hide-modules", "ram"], vec![Module::Os]),
            (
                vec!["--modules", "kernel", "--hide-modules", "kernel"],
                vec![Module::Kernel],
            ),
        ] {
            let mut cli = Cli::parse_from(std::iter::once("ferrisfetch").chain(args));
            let config: Config =
                serde_json::from_str(r#"{"modules":["ram","os"],"hide-modules":["os"]}"#).unwrap();
            config.apply(&mut cli).unwrap();
            assert_eq!(cli.selected_modules(), expected);
        }
    }

    #[test]
    fn scalar_settings_respect_explicit_cli_options() {
        for args in [
            vec![],
            vec!["--logo", "arch", "--cpu-usage-interval", "200"],
        ] {
            let explicit = !args.is_empty();
            let mut cli = Cli::parse_from(std::iter::once("ferrisfetch").chain(args));
            let config: Config = serde_json::from_str(
                r#"{"logo":"off","cpu-usage-interval":500,"no-color":true,"nerd-separator":true}"#,
            )
            .unwrap();
            config.apply(&mut cli).unwrap();
            assert_eq!(
                cli.logo.as_deref(),
                Some(if explicit { "arch" } else { "off" })
            );
            assert_eq!(
                cli.cpu_usage_interval,
                Some(if explicit { 200 } else { 500 })
            );
            assert!(cli.no_color);
            assert!(cli.nerd_separator);
        }
    }
}
