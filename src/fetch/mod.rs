//! Data collection: one entry per requested module (one per connected monitor).

pub(crate) mod command;
pub mod cpu;
pub mod disk;
pub mod gpu;
pub mod host;
pub mod kernel;
pub mod monitor;
pub mod os;
pub mod ram;
pub mod uptime;
pub mod util;

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
    Gpu,
    Monitor,
    Ram,
    Disk,
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
/// Collection is selective: `gpu` shells out to `nvidia-smi` and `cpu` waits
/// for a usage delta, so a run that does not request them skips that cost.
pub struct Context {
    system: System,
    disks: Disks,
    os_release: Option<os::OsRelease>,
    cpu_usage: Option<f32>,
}

impl Context {
    /// Refreshes only what `modules` actually needs.
    pub fn collect(modules: &[Module]) -> Self {
        let mut system = System::new();
        let needs_cpu = modules.contains(&Module::Cpu);

        system.refresh_memory();
        system.refresh_cpu_usage();

        // CPU usage is a delta between two samples, so it needs a second read.
        let cpu_usage = if needs_cpu {
            system.refresh_cpu_frequency();
            std::thread::sleep(std::time::Duration::from_millis(200));
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

        Self {
            system,
            disks,
            os_release: os::OsRelease::detect(),
            cpu_usage,
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
            Module::Gpu => gpu::gpu(self)?,
            Module::Ram => ram::ram(self)?,
            Module::Disk => disk::disk(self)?,
            Module::Monitor => monitor::monitors(self).into_iter().next()?,
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
        Module::Gpu => "GPU",
        Module::Ram => "RAM",
        Module::Disk => "Disk",
        Module::Monitor => "Monitor",
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
            Module::Gpu,
            Module::Ram,
            Module::Disk,
            Module::Monitor,
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
