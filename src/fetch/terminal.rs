//! Report the nearest terminal endpoint, not TERM's capabilities or a guess.

fn name(value: &str) -> Option<&'static str> {
    match value.to_ascii_lowercase().as_str() {
        "alacritty" => Some("Alacritty"),
        "kitty" => Some("kitty"),
        "ghostty" => Some("Ghostty"),
        "foot" | "footclient" => Some("foot"),
        "wezterm" | "wezterm-gui" | "wezterm-mux-server" => Some("WezTerm"),
        "gnome-terminal" | "gnome-terminal-server" => Some("GNOME Terminal"),
        "konsole" => Some("Konsole"),
        "xterm" => Some("xterm"),
        "urxvt" | "urxvtd" => Some("rxvt-unicode"),
        "st" => Some("st"),
        "tilix" => Some("Tilix"),
        "xfce4-terminal" => Some("Xfce Terminal"),
        "terminator" => Some("Terminator"),
        "contour" => Some("Contour"),
        "rio" => Some("Rio"),
        "vscode" | "code" => Some("Visual Studio Code"),
        "apple_terminal" => Some("Apple Terminal"),
        "iterm.app" => Some("iTerm2"),
        _ => None,
    }
}

pub(super) fn detect(parents: &[String], env: impl Fn(&str) -> Option<String>) -> Option<String> {
    let ssh =
        env("SSH_CONNECTION").is_some() || env("SSH_TTY").is_some() || env("SSH_CLIENT").is_some();
    let mut ancestor = None;
    // Stop at the nearest endpoint. In SSH, inherited local hints are not
    // evidence for the remote terminal. A multiplexer inside SSH is valid.
    for process in parents {
        let value = process.as_str();
        if value == "tmux" || value.starts_with("tmux:") {
            return Some("tmux".into());
        }
        if matches!(value, "screen" | "SCREEN") {
            return Some("screen".into());
        }
        if value == "sshd" || value.starts_with("sshd:") {
            break;
        }
        // SSH environment also blocks misleading outer local ancestors (ssh
        // clients in test/launcher chains), but not multiplexers above.
        if !ssh
            && ancestor.is_none()
            && let Some(terminal) = name(value)
        {
            ancestor = Some(terminal);
            break;
        }
    }
    if env("TMUX").is_some() {
        return Some("tmux".into());
    }
    if env("STY").is_some() {
        return Some("screen".into());
    }
    if ssh
        || parents
            .iter()
            .any(|p| p == "sshd" || p.starts_with("sshd:"))
    {
        return None;
    }
    ancestor
        .or_else(|| env("TERM_PROGRAM").as_deref().and_then(name))
        .or_else(|| {
            if env("KITTY_WINDOW_ID").is_some() {
                Some("kitty")
            } else if env("KONSOLE_VERSION").is_some() {
                Some("Konsole")
            } else if env("GHOSTTY_RESOURCES_DIR").is_some() {
                Some("Ghostty")
            } else if env("WEZTERM_PANE").is_some() {
                Some("WezTerm")
            } else {
                None
            }
        })
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(parents: &[&str], vars: &[(&str, &str)]) -> Option<String> {
        detect(
            &parents.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            |key| {
                vars.iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| v.to_string())
            },
        )
    }

    #[test]
    fn ancestry_and_terminal_hints() {
        assert_eq!(
            fixture(&["sudo", "bash", "kitty"], &[]).as_deref(),
            Some("kitty")
        );
        assert_eq!(
            fixture(&[], &[("TERM_PROGRAM", "WezTerm")]).as_deref(),
            Some("WezTerm")
        );
        assert_eq!(
            fixture(&[], &[("KITTY_WINDOW_ID", "1")]).as_deref(),
            Some("kitty")
        );
        assert_eq!(fixture(&["footclient"], &[]).as_deref(), Some("foot"));
    }

    #[test]
    fn multiplexers_and_ssh_are_boundaries() {
        assert_eq!(
            fixture(&["bash", "tmux: server", "kitty"], &[]).as_deref(),
            Some("tmux")
        );
        assert_eq!(
            fixture(&["SCREEN", "xterm"], &[]).as_deref(),
            Some("screen")
        );
        assert_eq!(
            fixture(&[], &[("TMUX", "/socket,1,0"), ("SSH_TTY", "/dev/pts/1")]).as_deref(),
            Some("tmux")
        );
        assert_eq!(
            fixture(&["sshd", "xterm"], &[("TERM_PROGRAM", "kitty")]),
            None
        );
        assert_eq!(fixture(&["kitty"], &[("SSH_CONNECTION", "remote")]), None);
        assert_eq!(
            fixture(&[], &[("STY", "123.pts")]).as_deref(),
            Some("screen")
        );
    }

    #[test]
    fn term_and_generic_vte_are_not_emulator_names() {
        for vars in [
            vec![],
            vec![("TERM", "xterm-256color")],
            vec![("TERM", "screen")],
            vec![("VTE_VERSION", "8000")],
            vec![("TERM_PROGRAM", "unknown")],
        ] {
            assert_eq!(fixture(&[], &vars), None);
        }
    }
}
