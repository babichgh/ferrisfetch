//! Desktop environment names only; compositor names are deliberately excluded.

fn normalize(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "gnome" | "gnome-classic" | "ubuntu" | "ubuntu-xorg" | "ubuntu-wayland" => Some("GNOME"),
        "kde" | "plasma" | "plasmax11" | "plasmawayland" => Some("KDE Plasma"),
        "xfce" | "xfce4" => Some("Xfce"),
        "cinnamon" | "x-cinnamon" => Some("Cinnamon"),
        "mate" => Some("MATE"),
        "lxqt" => Some("LXQt"),
        "lxde" => Some("LXDE"),
        "budgie" | "budgie-desktop" => Some("Budgie"),
        "unity" => Some("Unity"),
        "pantheon" => Some("Pantheon"),
        "deepin" | "dde" => Some("Deepin"),
        "cosmic" => Some("COSMIC"),
        _ => None,
    }
}

fn desktop(value: &str) -> Option<String> {
    let mut names = Vec::new();
    for part in value.split(':') {
        if let Some(name) = normalize(part)
            && !names.contains(&name)
        {
            names.push(name);
        }
    }
    (!names.is_empty()).then(|| names.join(" / "))
}

pub(super) fn detect(env: impl Fn(&str) -> Option<String>) -> Option<String> {
    env("XDG_CURRENT_DESKTOP")
        .as_deref()
        .and_then(desktop)
        .or_else(|| env("XDG_SESSION_DESKTOP").as_deref().and_then(desktop))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_and_composite_fixtures() {
        for (raw, expected) in [
            ("ubuntu:GNOME", "GNOME"),
            ("KDE", "KDE Plasma"),
            (" X-Cinnamon ", "Cinnamon"),
            ("Budgie:GNOME", "Budgie / GNOME"),
            ("GNOME:gnome", "GNOME"),
            ("XFCE", "Xfce"),
        ] {
            assert_eq!(desktop(raw).as_deref(), Some(expected));
        }
        for raw in ["", "unknown", "Hyprland", "sway", "KWin"] {
            assert_eq!(desktop(raw), None);
        }
    }

    #[test]
    fn current_then_session_fallback() {
        assert_eq!(
            detect(|key| Some(
                if key == "XDG_CURRENT_DESKTOP" {
                    "unknown"
                } else {
                    "plasmawayland"
                }
                .into()
            ))
            .as_deref(),
            Some("KDE Plasma")
        );
        assert_eq!(
            detect(|key| Some(
                if key == "XDG_CURRENT_DESKTOP" {
                    "GNOME"
                } else {
                    "KDE"
                }
                .into()
            ))
            .as_deref(),
            Some("GNOME")
        );
        assert_eq!(detect(|_| None), None);
    }
}
