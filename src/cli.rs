//! Command line interface.

use crate::fetch::Module;
use crate::render::{Layout, NERD_SEPARATOR, SEPARATOR};
use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "ferrisfetch",
    version,
    about = "A fastfetch clone written in Rust",
    disable_help_subcommand = true
)]
pub struct Cli {
    /// Modules to show, in the given order (default: standard set, without cpu-usage)
    #[arg(short, long, value_enum, value_delimiter = ',', num_args = 1..)]
    pub modules: Option<Vec<Module>>,

    /// Modules to hide from the default set
    #[arg(short = 'u', long, value_enum, value_delimiter = ',', num_args = 1..)]
    pub hide_modules: Option<Vec<Module>>,

    /// CPU usage sampling interval in milliseconds (minimum/default: 200); only for cpu-usage
    #[arg(long, value_name = "MS", value_parser = clap::value_parser!(u64).range(200..))]
    pub cpu_usage_interval: Option<u64>,

    /// Hide the logo and print a single column
    #[arg(long)]
    pub logo_off: bool,

    /// Force a specific logo (fedora, arch, debian, ...) or "off"
    #[arg(long)]
    pub logo: Option<String>,

    /// Disable ANSI colour output
    #[arg(long)]
    pub no_color: bool,

    /// Use the Nerd Font separator instead of ':'
    #[arg(long)]
    pub nerd_separator: bool,

    /// Plain output, one `Key: value` per line, for scripts
    #[arg(long)]
    pub pipe: bool,

    /// JSON output, one object per field
    #[arg(long)]
    pub json: bool,

    /// List every available logo name and exit
    #[arg(long)]
    pub logo_list: bool,
}

impl Cli {
    /// Resolves the effective module list from defaults plus `--hide-modules`.
    pub fn selected_modules(&self) -> Vec<Module> {
        if let Some(m) = &self.modules {
            return dedup(m);
        }
        if let Some(hide) = &self.hide_modules {
            let hide = dedup(hide);
            return crate::fetch::DEFAULT_ORDER
                .iter()
                .copied()
                .filter(|m| !hide.contains(m))
                .collect();
        }
        crate::fetch::DEFAULT_ORDER.to_vec()
    }

    /// Colour is on only for a TTY with no opt-out.
    pub fn colors_enabled(&self) -> bool {
        !self.no_color
            && std::env::var_os("NO_COLOR").is_none()
            && std::io::IsTerminal::is_terminal(&std::io::stdout())
    }

    pub fn layout(&self) -> Layout {
        Layout {
            separator: if self.nerd_separator {
                NERD_SEPARATOR
            } else {
                SEPARATOR
            },
            ..Layout::default()
        }
    }

    /// Terminal width, when stdout is attached to a terminal.
    pub fn term_width(&self) -> Option<usize> {
        if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
            return None;
        }
        terminal_size::terminal_size().map(|(terminal_size::Width(w), _)| w as usize)
    }
}

/// Keeps the first occurrence of each module, preserving the requested order.
fn dedup(modules: &[Module]) -> Vec<Module> {
    let mut out: Vec<Module> = Vec::with_capacity(modules.len());
    for m in modules {
        if !out.contains(m) {
            out.push(*m);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("ferrisfetch").chain(args.iter().copied())).unwrap()
    }

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn no_args_uses_default_order() {
        assert_eq!(parse(&[]).selected_modules(), crate::fetch::DEFAULT_ORDER);
    }

    #[test]
    fn explicit_order_is_preserved() {
        let cli = parse(&["--modules", "ram,cpu"]);
        assert_eq!(cli.selected_modules(), vec![Module::Ram, Module::Cpu]);
    }

    #[test]
    fn duplicate_modules_are_removed() {
        let cli = parse(&["--modules", "ram,ram,cpu"]);
        assert_eq!(cli.selected_modules(), vec![Module::Ram, Module::Cpu]);
    }

    #[test]
    fn cpu_usage_is_selectable_but_not_default() {
        assert!(!parse(&[]).selected_modules().contains(&Module::CpuUsage));
        assert_eq!(
            parse(&["--modules", "cpu,cpu-usage"]).selected_modules(),
            vec![Module::Cpu, Module::CpuUsage]
        );
    }

    #[test]
    fn cpu_usage_interval_is_validated() {
        assert_eq!(parse(&[]).cpu_usage_interval, None);
        assert_eq!(
            parse(&["--cpu-usage-interval", "500"]).cpu_usage_interval,
            Some(500)
        );
        assert_eq!(
            parse(&["--cpu-usage-interval", "200"]).cpu_usage_interval,
            Some(200)
        );
        for value in ["0", "199", "-1", "abc"] {
            assert!(Cli::try_parse_from(["ferrisfetch", "--cpu-usage-interval", value]).is_err());
        }
    }

    #[test]
    fn hide_removes_from_default_set() {
        let cli = parse(&["--hide-modules", "gpu,disk"]);
        let sel = cli.selected_modules();
        assert!(!sel.contains(&Module::Gpu));
        assert!(!sel.contains(&Module::Disk));
        assert!(sel.contains(&Module::Os));
    }

    #[test]
    fn explicit_modules_win_over_hide() {
        let cli = parse(&["--modules", "os", "--hide-modules", "os"]);
        assert_eq!(cli.selected_modules(), vec![Module::Os]);
    }

    #[test]
    fn separator_defaults_to_ascii_colon() {
        assert_eq!(parse(&[]).layout().separator, SEPARATOR);
    }

    #[test]
    fn nerd_separator_flag_switches_glyph() {
        assert_eq!(
            parse(&["--nerd-separator"]).layout().separator,
            NERD_SEPARATOR
        );
    }

    #[test]
    fn unknown_module_is_rejected() {
        let res = Cli::try_parse_from(["ferrisfetch", "--modules", "nonsense"]);
        assert!(res.is_err());
    }

    #[test]
    fn no_color_flag_disables_color() {
        assert!(!parse(&["--no-color"]).colors_enabled());
    }

    #[test]
    fn plain_and_json_are_flagged() {
        assert!(parse(&["--pipe"]).pipe);
        assert!(parse(&["--json"]).json);
        assert!(parse(&["--logo-list"]).logo_list);
        // `--logo off` is distinct from the `--logo-off` flag.
        assert!(!parse(&["--logo", "off"]).logo_off);
        assert!(parse(&["--logo-off"]).logo_off);
    }
}
