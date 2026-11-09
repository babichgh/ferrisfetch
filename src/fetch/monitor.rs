//! Active monitors, using the compositor's current mode rather than an EDID
//! mode that may not actually be in use.

use crate::fetch::Context;

struct Monitor {
    name: String,
    width: u64,
    height: u64,
    width_mm: u64,
    height_mm: u64,
    hz: f64,
}

impl Monitor {
    fn display(&self) -> String {
        let mut text = format!("{}: {}x{}", self.name, self.width, self.height);
        if self.width_mm > 0 && self.height_mm > 0 {
            let diagonal = (self.width_mm as f64).hypot(self.height_mm as f64) / 25.4;
            text.push_str(&format!(" ({diagonal:.1}\")"));
        }
        if self.hz.is_finite() && self.hz > 0.0 {
            text.push_str(&format!(" @ {:.2} Hz", self.hz));
        }
        text
    }
}

/// Hyprland reports the actual active mode and physical size in millimetres.
fn parse_hyprland(raw: &str) -> Option<Vec<Monitor>> {
    let monitors = serde_json::from_str::<serde_json::Value>(raw).ok()?;
    Some(
        monitors
            .as_array()?
            .iter()
            .filter(|m| m["disabled"] != true)
            .filter_map(|m| {
                let name = m["name"].as_str()?.to_owned();
                let width = m["width"].as_u64()?;
                let height = m["height"].as_u64()?;
                if name.is_empty() || width == 0 || height == 0 {
                    return None;
                }
                Some(Monitor {
                    name,
                    width,
                    height,
                    width_mm: m["physicalWidth"].as_u64().unwrap_or(0),
                    height_mm: m["physicalHeight"].as_u64().unwrap_or(0),
                    hz: m["refreshRate"].as_f64().unwrap_or(0.0),
                })
            })
            .collect(),
    )
}

fn hyprland() -> Option<Vec<Monitor>> {
    parse_hyprland(&super::command::output("hyprctl", &["monitors", "-j"])?)
}

/// XRandR marks the current refresh rate with `*`; the connector line gives
/// its physical size. Ignore disconnected connectors and other available modes.
fn parse_xrandr(raw: &str) -> Vec<Monitor> {
    let mut monitors: Vec<Monitor> = Vec::new();
    let mut active = None;
    for line in raw.lines() {
        if !line.starts_with([' ', '\t']) {
            active = None;
            let words: Vec<&str> = line.split_whitespace().collect();
            if words.get(1) != Some(&"connected") {
                continue;
            }
            let Some(mode) = words.iter().find(|w| w.contains('x') && w.contains('+')) else {
                continue;
            };
            let Some((width, rest)) = mode.split_once('x') else {
                continue;
            };
            let Some((height, _)) = rest.split_once('+') else {
                continue;
            };
            let (Ok(width), Ok(height)) = (width.parse(), height.parse()) else {
                continue;
            };
            let mm = words.windows(3).find_map(|w| {
                Some((
                    w[0].strip_suffix("mm")?.parse().ok()?,
                    (w[1] == "x")
                        .then_some(w[2])?
                        .strip_suffix("mm")?
                        .parse()
                        .ok()?,
                ))
            });
            let (width_mm, height_mm) = mm.unwrap_or((0, 0));
            monitors.push(Monitor {
                name: words[0].to_owned(),
                width,
                height,
                width_mm,
                height_mm,
                hz: 0.0,
            });
            active = Some(monitors.len() - 1);
        } else if let Some(monitor) = active.and_then(|index| monitors.get_mut(index)) {
            let mut words = line.split_whitespace();
            if words.next() == Some(format!("{}x{}", monitor.width, monitor.height).as_str()) {
                for rate in words {
                    if rate.contains('*') {
                        monitor.hz = rate.trim_end_matches(['*', '+']).parse().unwrap_or(0.0);
                        break;
                    }
                }
            }
        }
    }
    monitors
}

fn xrandr() -> Option<Vec<Monitor>> {
    Some(parse_xrandr(&super::command::output(
        "xrandr",
        &["--query"],
    )?))
}

pub fn monitors(_ctx: &Context) -> Vec<String> {
    hyprland()
        .or_else(xrandr)
        .unwrap_or_default()
        .iter()
        .map(Monitor::display)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hyprland_uses_current_mode_and_skips_disabled() {
        let raw = r#"[
            {"name":"DP-1","width":1920,"height":1080,
             "physicalWidth":530,"physicalHeight":300,"refreshRate":59.94,
             "availableModes":["1920x1080@100Hz"]},
            {"name":"HDMI-A-1","width":1280,"height":720,"disabled":true}
        ]"#;
        let monitors = parse_hyprland(raw).unwrap();
        assert_eq!(monitors.len(), 1);
        assert_eq!(monitors[0].display(), "DP-1: 1920x1080 (24.0\") @ 59.94 Hz");
    }

    #[test]
    fn missing_physical_size_omits_inches() {
        let monitors =
            parse_hyprland(r#"[{"name":"eDP-1","width":2560,"height":1440,"refreshRate":60}]"#)
                .unwrap();
        assert_eq!(monitors[0].display(), "eDP-1: 2560x1440 @ 60.00 Hz");
        assert!(parse_hyprland("not json").is_none());
    }

    #[test]
    fn xrandr_uses_starred_refresh_rate() {
        let raw = "Screen 0: minimum 8 x 8, current 1920 x 1080\nDP-1 connected primary 1920x1080+0+0 (normal) 530mm x 300mm\n   1920x1080  60.00*+  99.65\nHDMI-1 disconnected (normal)\n";
        let monitors = parse_xrandr(raw);
        assert_eq!(monitors.len(), 1);
        assert_eq!(monitors[0].display(), "DP-1: 1920x1080 (24.0\") @ 60.00 Hz");
    }
}
