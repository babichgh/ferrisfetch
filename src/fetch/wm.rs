//! Compositor/WM detection, not inference from the desktop environment name.

use super::command;

pub(super) fn detect(env: impl Fn(&str) -> Option<String>) -> Option<String> {
    if let Some(name) = identify(&env) {
        return Some(name.into());
    }
    // KWin is commonly launched by systemd, not an ancestor of the terminal.
    // Query its own D-Bus service, only in a local KDE graphical session.
    let tool = kwin_probe(&env)?;
    let output = command::output(
        tool,
        &["org.kde.KWin", "/KWin", "org.kde.KWin.supportInformation"],
    )?;
    parse_kwin(&output).map(str::to_string)
}

fn identify(env: &impl Fn(&str) -> Option<String>) -> Option<&'static str> {
    if env("SSH_CONNECTION").is_some() || env("SSH_TTY").is_some() || env("SSH_CLIENT").is_some() {
        return None;
    }
    if env("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        return Some("Hyprland");
    }
    if env("SWAYSOCK").is_some() {
        return Some("Sway");
    }
    None
}

fn kwin_probe(env: &impl Fn(&str) -> Option<String>) -> Option<&'static str> {
    if env("SSH_CONNECTION").is_some()
        || env("SSH_TTY").is_some()
        || env("SSH_CLIENT").is_some()
        || (env("WAYLAND_DISPLAY").is_none() && env("DISPLAY").is_none())
    {
        return None;
    }
    let kde = ["XDG_CURRENT_DESKTOP", "XDG_SESSION_DESKTOP"]
        .iter()
        .any(|key| {
            env(key).is_some_and(|v| {
                v.split(':').any(|part| {
                    matches!(
                        part.trim().to_ascii_lowercase().as_str(),
                        "kde" | "plasma" | "plasmawayland" | "plasmax11"
                    )
                })
            })
        });
    kde.then(|| {
        if env("KDE_SESSION_VERSION").as_deref() == Some("5") {
            "qdbus"
        } else {
            "qdbus6"
        }
    })
}

fn parse_kwin(output: &str) -> Option<&'static str> {
    output
        .lines()
        .any(|l| l.trim() == "Name: KWin")
        .then_some("KWin")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compositor_fixtures_do_not_confuse_de_and_wm() {
        for (key, expected) in [
            ("HYPRLAND_INSTANCE_SIGNATURE", "Hyprland"),
            ("SWAYSOCK", "Sway"),
        ] {
            assert_eq!(
                identify(&|k| (k == key).then(|| "value".into())),
                Some(expected)
            );
        }
        assert_eq!(
            identify(&|k| (k == "XDG_CURRENT_DESKTOP").then(|| "KDE".into())),
            None
        );
        assert_eq!(
            identify(&|k| match k {
                "SWAYSOCK" => Some("socket".into()),
                "SSH_TTY" => Some("pts".into()),
                _ => None,
            }),
            None
        );
    }

    #[test]
    fn only_probe_kwin_in_matching_graphical_session() {
        let env = |k: &str| match k {
            "XDG_CURRENT_DESKTOP" => Some("KDE".into()),
            "DISPLAY" => Some(":0".into()),
            _ => None,
        };
        assert_eq!(kwin_probe(&env), Some("qdbus6"));
        assert_eq!(
            kwin_probe(&|k| if k == "KDE_SESSION_VERSION" {
                Some("5".into())
            } else {
                env(k)
            }),
            Some("qdbus")
        );
        for blocked in ["SSH_CONNECTION", "SSH_TTY"] {
            assert_eq!(
                kwin_probe(&|k| if k == blocked {
                    Some("remote".into())
                } else {
                    env(k)
                }),
                None
            );
        }
        assert_eq!(
            kwin_probe(&|k| (k == "XDG_CURRENT_DESKTOP").then(|| "KDE".into())),
            None
        );
        assert_eq!(kwin_probe(&|k| (k == "DISPLAY").then(|| ":0".into())), None);
    }

    #[test]
    fn kwin_response_must_identify_itself() {
        assert_eq!(
            parse_kwin("KWin Support Information:\nName: KWin\nVersion: 6.5\n"),
            Some("KWin")
        );
        for bad in [
            "",
            "Name: Mutter",
            "error: KWin unavailable",
            "Name: KWinFake",
        ] {
            assert_eq!(parse_kwin(bad), None);
        }
    }
}
