//! Small, shared I/O boundary for session detectors. Never enumerate /proc.

use std::collections::HashSet;

pub(super) fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Process {
    pub parent: u32,
    pub name: String,
}

/// The command in stat may contain spaces and parentheses. PPID follows state.
fn parse_stat(stat: &str) -> Option<Process> {
    let start = stat.find('(')?;
    let end = stat.rfind(')')?;
    let name = stat.get(start + 1..end)?.to_string();
    let parent = stat
        .get(end + 1..)?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    Some(Process { parent, name })
}

fn read_process(pid: u32) -> Option<Process> {
    // Status is independent of stat/exe access; retain PPID even when the
    // executable is unavailable (hidepid, a process exiting, or permissions).
    let base = format!("/proc/{pid}");
    let stat = std::fs::read_to_string(format!("{base}/stat"))
        .ok()
        .and_then(|s| parse_stat(&s));
    let mut process = stat.or_else(|| {
        let status = std::fs::read_to_string(format!("{base}/status")).ok()?;
        parse_status(&status)
    })?;
    if let Ok(exe) = std::fs::read_link(format!("{base}/exe"))
        && let Some(name) = exe.file_name().and_then(|s| s.to_str())
    {
        process.name = name.trim_end_matches(" (deleted)").to_string();
    }
    Some(process)
}

fn parse_status(status: &str) -> Option<Process> {
    let field = |key: &str| {
        status
            .lines()
            .find_map(|l| l.strip_prefix(key))
            .map(str::trim)
    };
    Some(Process {
        parent: field("PPid:")?.parse().ok()?,
        name: field("Name:").unwrap_or_default().to_string(),
    })
}

/// Nearest parent first; bounded even for malformed/cyclic fixtures.
fn walk(start: u32, mut read: impl FnMut(u32) -> Option<Process>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut names = Vec::new();
    let mut pid = start;
    for depth in 0..64 {
        if pid == 0 || !seen.insert(pid) {
            break;
        }
        let Some(process) = read(pid) else { break };
        if depth != 0 {
            names.push(process.name);
        }
        pid = process.parent;
    }
    names
}

pub(super) fn parents() -> Vec<String> {
    walk(std::process::id(), read_process)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proc_fixtures() {
        assert_eq!(
            parse_stat("42 (a (b) c) S 7 0 0").unwrap(),
            Process {
                parent: 7,
                name: "a (b) c".into(),
            }
        );
        for bad in ["", "42 (bash) S nope", "42 bash S 1", "42 ) ("] {
            assert!(parse_stat(bad).is_none());
        }
        assert_eq!(parse_status("Name:\tbash\nPPid:\t12\n").unwrap().parent, 12);
        assert!(parse_status("Name: bash\n").is_none());
        assert_eq!(parse_status("PPid: 12\n").unwrap().name, "");
    }

    #[test]
    fn missing_names_do_not_stop_walk_but_missing_parent_does() {
        let result = walk(10, |pid| match pid {
            10 => Some(Process {
                parent: 9,
                name: "self".into(),
            }),
            9 => Some(Process {
                parent: 8,
                name: String::new(),
            }),
            8 => Some(Process {
                parent: 7,
                name: "bash".into(),
            }),
            _ => None,
        });
        assert_eq!(result, ["", "bash"]);
        assert!(walk(10, |_| None).is_empty());
    }

    #[test]
    fn cycles_and_depth_are_bounded() {
        assert_eq!(
            walk(2, |_| Some(Process {
                parent: 2,
                name: "x".into()
            }))
            .len(),
            0
        );
        assert_eq!(
            walk(100, |pid| Some(Process {
                parent: pid - 1,
                name: "x".into()
            }))
            .len(),
            63
        );
    }
}
