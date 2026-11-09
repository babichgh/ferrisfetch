//! Data collection: one entry per requested module (one per connected monitor).

pub(crate) mod command;
pub mod cpu;
pub mod cpu_usage;
mod de;
pub mod disk;
pub mod gpu;
pub mod host;
pub mod kernel;
pub mod monitor;
pub mod os;
pub mod ram;
mod session;
mod shell;
mod terminal;
pub mod uptime;
pub mod util;
mod wm;

use crate::render::Entry;
use clap::ValueEnum;
use sysinfo::{Disks, System};

/// A selectable piece of system information.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Module {
    Os,
    Host,
    Kernel,
    Uptime,
    Cpu,
    CpuUsage,
    Gpu,
    Monitor,
    Ram,
    Disk,
    Shell,
    Terminal,
    De,
    Wm,
}

/// Order used when the user does not pass `--modules`.
pub const DEFAULT_ORDER: &[Module] = &[
    Module::Os,
    Module::Host,
    Module::Kernel,
    Module::Uptime,
    Module::Cpu,
    Module::Gpu,
    Module::Monitor,
    Module::Ram,
    Module::Disk,
];

/// Everything the modules read from, refreshed exactly once per run.
///
/// Collection is selective: `gpu` shells out to `nvidia-smi` and `cpu-usage` waits
/// for a usage delta, so a run that does not request them skips that cost.
pub struct Context {
    system: System,
    disks: Disks,
    os_release: Option<os::OsRelease>,
    cpu_usage: Option<f32>,
    shell: Option<String>,
    terminal: Option<String>,
    de: Option<String>,
    wm: Option<String>,
}

impl Context {
    /// Refreshes only what `modules` actually needs.
    pub fn collect(modules: &[Module]) -> Self {
        Self::collect_with_cpu_interval(modules, cpu_usage::DEFAULT_INTERVAL)
    }

    /// The interval only applies when `cpu-usage` is selected.
    pub fn collect_with_cpu_interval(modules: &[Module], interval: std::time::Duration) -> Self {
        let mut system = System::new();

        if modules.contains(&Module::Ram) {
            system.refresh_memory();
        }
        if modules.contains(&Module::Cpu) {
            system.refresh_cpu_frequency();
        }

        // CPU usage is a delta between two samples, so it needs a second read.
        let cpu_usage = if modules.contains(&Module::CpuUsage) {
            system.refresh_cpu_usage();
            std::thread::sleep(interval.max(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL));
            system.refresh_cpu_usage();
            Some(system.global_cpu_usage())
        } else {
            None
        };

        // `new_with_refreshed_list` loads both the disk list and its counters.
        let disks = if modules.contains(&Module::Disk) {
            Disks::new_with_refreshed_list()
        } else {
            Disks::new()
        };

        let parents = if modules
            .iter()
            .any(|m| matches!(m, Module::Shell | Module::Terminal))
        {
            session::parents()
        } else {
            Vec::new()
        };

        Self {
            system,
            disks,
            os_release: os::OsRelease::detect(),
            cpu_usage,
            shell: modules
                .contains(&Module::Shell)
                .then(|| shell::detect(&parents, session::env("SHELL").as_deref()))
                .flatten(),
            terminal: modules
                .contains(&Module::Terminal)
                .then(|| terminal::detect(&parents, session::env))
                .flatten(),
            de: modules
                .contains(&Module::De)
                .then(|| de::detect(session::env))
                .flatten(),
            wm: modules
                .contains(&Module::Wm)
                .then(|| wm::detect(session::env))
                .flatten(),
        }
    }

    /// Renders one module, or `None` when the data is unavailable.
    ///
    /// Modules never fail: on a bare server or in CI the line is simply
    /// omitted instead of printing `unknown`.
    pub fn entry(&self, module: Module) -> Option<Entry> {
        let value = match module {
            Module::Os => os::os(self)?,
            Module::Host => host::host(self)?,
            Module::Kernel => kernel::kernel(self)?,
            Module::Uptime => uptime::uptime(self)?,
            Module::Cpu => cpu::cpu(self)?,
            Module::CpuUsage => cpu_usage::cpu_usage(self)?,
            Module::Gpu => gpu::gpu(self)?,
            Module::Ram => ram::ram(self)?,
            Module::Disk => disk::disk(self)?,
            Module::Monitor => monitor::monitors(self).into_iter().next()?,
            Module::Shell => self.shell.clone()?,
            Module::Terminal => self.terminal.clone()?,
            Module::De => self.de.clone()?,
            Module::Wm => self.wm.clone()?,
        };
        let value = value.trim();
        if value.is_empty() {
            return None;
        }
        Some(Entry {
            key: label(module).to_string(),
            value: value.to_string(),
        })
    }

    /// Collects every requested module, skipping unavailable ones.
    pub fn entries(&self, modules: &[Module]) -> Vec<Entry> {
        modules
            .iter()
            .flat_map(|m| {
                if *m == Module::Monitor {
                    monitor_entries(monitor::monitors(self))
                } else {
                    self.entry(*m).into_iter().collect()
                }
            })
            .collect()
    }

    /// `/etc/os-release` ID, used to pick the logo.
    pub fn os_id(&self) -> Option<&str> {
        self.os_release.as_ref()?.id.as_deref()
    }
}

fn monitor_entries(values: Vec<String>) -> Vec<Entry> {
    values
        .into_iter()
        .enumerate()
        .map(|(i, value)| Entry {
            key: if i == 0 {
                "Monitor".to_string()
            } else {
                format!("Monitor {}", i + 1)
            },
            value,
        })
        .collect()
}

/// Human-facing key for a module.
pub fn label(module: Module) -> &'static str {
    match module {
        Module::Os => "OS",
        Module::Host => "Host",
        Module::Kernel => "Kernel",
        Module::Uptime => "Uptime",
        Module::Cpu => "CPU",
        Module::CpuUsage => "CPU Usage",
        Module::Gpu => "GPU",
        Module::Ram => "RAM",
        Module::Disk => "Disk",
        Module::Monitor => "Monitor",
        Module::Shell => "Shell",
        Module::Terminal => "Terminal",
        Module::De => "DE",
        Module::Wm => "WM",
    }
}

pub(crate) fn system(ctx: &Context) -> &System {
    &ctx.system
}

pub(crate) fn disks(ctx: &Context) -> &Disks {
    &ctx.disks
}

pub(crate) fn cpu_usage(ctx: &Context) -> Option<f32> {
    ctx.cpu_usage
}

pub(crate) fn os_release(ctx: &Context) -> Option<&os::OsRelease> {
    ctx.os_release.as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_module_has_a_label() {
        for m in [
            Module::Os,
            Module::Host,
            Module::Kernel,
            Module::Uptime,
            Module::Cpu,
            Module::CpuUsage,
            Module::Gpu,
            Module::Ram,
            Module::Disk,
            Module::Monitor,
            Module::Shell,
            Module::Terminal,
            Module::De,
            Module::Wm,
        ] {
            assert!(!label(m).is_empty());
        }
    }

    #[test]
    fn default_order_has_no_duplicates() {
        for (i, a) in DEFAULT_ORDER.iter().enumerate() {
            for b in &DEFAULT_ORDER[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn context_collects_and_omits_missing_modules() {
        let ctx = Context::collect(DEFAULT_ORDER);
        let entries = ctx.entries(DEFAULT_ORDER);
        // At minimum the host must resolve; nothing may panic.
        for e in &entries {
            assert!(!e.key.is_empty());
            assert!(!e.value.is_empty());
        }
    }

    #[test]
    fn requesting_no_modules_is_cheap_and_empty() {
        let ctx = Context::collect(&[]);
        assert!(ctx.entries(&[]).is_empty());
        assert!(ctx.cpu_usage.is_none());
        assert!(ctx.system.cpus().is_empty());
        assert_eq!(ctx.system.total_memory(), 0);
        assert!(ctx.disks.is_empty());
        assert!(ctx.shell.is_none());
        assert!(ctx.terminal.is_none());
        assert!(ctx.de.is_none());
        assert!(ctx.wm.is_none());
    }

    #[test]
    fn cpu_info_does_not_sample_usage_or_refresh_memory() {
        let ctx = Context::collect(&[Module::Cpu]);
        assert!(ctx.cpu_usage.is_none());
        assert!(!ctx.system.cpus().is_empty());
        assert_eq!(ctx.system.total_memory(), 0);
    }

    #[test]
    fn ram_does_not_refresh_cpus() {
        let ctx = Context::collect(&[Module::Ram]);
        assert!(ctx.system.cpus().is_empty());
        assert!(ctx.cpu_usage.is_none());
    }

    #[test]
    fn cpu_usage_is_opt_in() {
        assert!(!DEFAULT_ORDER.contains(&Module::CpuUsage));
        let ctx = Context::collect(&[Module::CpuUsage]);
        assert!(ctx.cpu_usage.is_some());
        assert_eq!(ctx.system.total_memory(), 0);
    }

    #[test]
    fn session_modules_are_opt_in() {
        for module in [Module::Shell, Module::Terminal, Module::De, Module::Wm] {
            assert!(!DEFAULT_ORDER.contains(&module));
        }
    }

    #[test]
    fn monitors_are_separate_entries_in_order() {
        let entries = monitor_entries(vec!["DP-1: 1920x1080".into(), "HDMI-1: 1024x768".into()]);
        assert_eq!(entries[0].key, "Monitor");
        assert_eq!(entries[1].key, "Monitor 2");
        assert_eq!(entries[1].value, "HDMI-1: 1024x768");
    }

    #[test]
    fn default_order_puts_monitors_between_gpu_and_ram() {
        let gpu = DEFAULT_ORDER
            .iter()
            .position(|m| *m == Module::Gpu)
            .unwrap();
        assert_eq!(DEFAULT_ORDER[gpu + 1], Module::Monitor);
        assert_eq!(DEFAULT_ORDER[gpu + 2], Module::Ram);
    }
}
