//! Logo artwork, vendored from fastfetch.
//!
//! The art under `logos/` is taken verbatim from fastfetch's `src/logo/ascii/`
//! (MIT, see LICENSE-fastfetch). `tools/generate_logos.py` turns fastfetch's C
//! headers into `generated.rs`, pairing each `.txt` with its display names and
//! colour palette.
//!
//! Art still contains `$N` markers that index the logo's palette. They are
//! resolved once at load time into ANSI escapes, or dropped entirely when
//! colour is off.

mod generated;

pub use generated::{LOGOS, RawLogo};

use crate::render::color::Style;

/// A logo prepared for rendering: `$N` already resolved, width precomputed.
pub struct Logo {
    lines: Vec<String>,
    width: usize,
}

impl Logo {
    /// Resolves `$N` markers against `palette`.
    ///
    /// With `color` off the markers are dropped entirely, so piped output
    /// stays plain.
    pub fn new(art: &str, palette: &[(u8, u8, u8)], color: bool) -> Self {
        let lines = render_art(art, palette, color);
        let width = lines
            .iter()
            .map(|l| crate::render::width::visible_len(l))
            .max()
            .unwrap_or(0);
        Self { lines, width }
    }

    /// Widest visible line.
    pub fn width(&self) -> usize {
        self.width
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }
}

const RESET: &str = "\x1b[0m";

/// Resolves fastfetch's `$N` markers into ANSI escapes.
///
/// Semantics copied from `ffLogoPrintChars` in fastfetch's `src/logo/logo.c`,
/// because guessing them gets subtle cases wrong:
///
/// * `$` is followed by exactly **one** character, not a number, so `$2888`
///   means "palette entry 2, then the literal text 888";
/// * `$$` is an escaped literal `$`;
/// * a `$` before anything else stays literal;
/// * an out-of-range index leaves the `$` as text and the digit is copied
///   normally;
/// * the active colour carries over to the following line, starting from
///   palette entry 0.
fn render_art(art: &str, palette: &[(u8, u8, u8)], color: bool) -> Vec<String> {
    let mut carry = 0usize;
    let mut out = Vec::new();

    for line in art.lines() {
        // Upstream art has stray trailing spaces; they would only skew the
        // width used for column padding.
        let mut s = String::with_capacity(line.len() + 16);
        if color {
            s.push_str(&escape(palette, carry));
        }

        let mut chars = line.trim_end().chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                // fastfetch expands tabs rather than emitting them.
                '\t' => s.push_str("    "),
                '$' => match chars.peek().copied() {
                    Some('$') => {
                        chars.next();
                        s.push('$');
                    }
                    Some(d @ '1'..='9') => {
                        chars.next();
                        let idx = d as usize - '1' as usize;
                        if color {
                            if palette.get(idx).is_some() {
                                s.push_str(&escape(palette, idx));
                                carry = idx;
                            } else {
                                s.push('$');
                                s.push(d);
                            }
                        }
                        // Colour off: drop both characters, as fastfetch's
                        // pipe mode does.
                    }
                    _ => s.push('$'),
                },
                c => s.push(c),
            }
        }

        if color {
            s.push_str(RESET);
        }
        out.push(s);
    }

    out
}

fn escape(palette: &[(u8, u8, u8)], index: usize) -> String {
    match palette.get(index) {
        Some(&(r, g, b)) => format!("\x1b[38;2;{r};{g};{b}m"),
        None => String::new(),
    }
}

/// Normalises a user-supplied or `os-release` ID for lookup.
fn normalize(name: &str) -> String {
    name.trim().to_ascii_lowercase().replace(['-', ' '], "_")
}

/// Finds a logo by any of its names or aliases.
pub fn find(name: &str) -> Option<&'static RawLogo> {
    let key = normalize(name);
    if key.is_empty() {
        return None;
    }
    LOGOS.iter().find(|l| l.names.iter().any(|n| *n == key))
}

/// The fallback artwork, used when no logo matches.
pub fn unknown() -> &'static RawLogo {
    LOGOS
        .iter()
        .find(|l| l.names.contains(&"unknown"))
        .expect("generated table always contains the unknown logo")
}

/// Picks the logo for a running system, falling back to `unknown`.
pub fn detect(os_id: Option<&str>) -> &'static RawLogo {
    os_id.and_then(find).unwrap_or_else(unknown)
}

/// Prepares a raw logo for rendering under the active style.
pub fn build(raw: &RawLogo, style: &Style) -> Logo {
    Logo::new(raw.art, raw.colors, style.colors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_populated() {
        assert!(
            LOGOS.len() > 500,
            "expected the full set, got {}",
            LOGOS.len()
        );
    }

    #[test]
    fn every_logo_has_art_and_a_name() {
        for l in LOGOS {
            assert!(!l.art.trim().is_empty(), "empty art: {:?}", l.names);
            assert!(!l.colors.is_empty(), "no palette: {:?}", l.names);
            assert!(!l.names.is_empty(), "no names for art");
        }
    }

    #[test]
    fn trailing_whitespace_is_normalised_at_load() {
        // Upstream art has stray trailing spaces; they would skew the width
        // used for column padding.
        let style = Style::default();
        for l in LOGOS {
            let logo = build(l, &style);
            for line in logo.lines() {
                assert_eq!(
                    line.trim_end(),
                    line,
                    "{:?}: trailing whitespace survived",
                    l.names
                );
            }
        }
    }

    #[test]
    fn out_of_range_markers_render_as_text_not_panic() {
        // Upstream art is not always consistent: aoscosretro uses $3 with a
        // two-entry palette. fastfetch renders that as literal text, and so
        // must we - the point is that it degrades, not that the data is clean.
        for l in LOGOS {
            let style = Style::default();
            let _ = build(l, &style);
        }
        let out = render_art("$3x", &[(1, 1, 1), (2, 2, 2)], true)[0].clone();
        assert!(out.contains("$3x"), "{out:?}");
    }

    #[test]
    fn primary_names_are_unique() {
        let mut seen: Vec<&str> = Vec::new();
        for l in LOGOS {
            let primary = l.names[0];
            assert!(!seen.contains(&primary), "duplicate primary name {primary}");
            seen.push(primary);
        }
    }

    #[test]
    fn marker_selects_palette_entry() {
        let out = render_art("$1ab$2cd", &[(1, 2, 3), (4, 5, 6)], true)[0].clone();
        assert!(out.contains("\x1b[38;2;1;2;3m"));
        assert!(out.contains("\x1b[38;2;4;5;6m"));
        assert!(out.contains("ab"));
        assert!(!out.contains('$'));
    }

    #[test]
    fn marker_consumes_exactly_one_digit() {
        // fastfetch reads a single char after '$', so this is $2 then "888".
        let out = render_art("$2888", &[(1, 1, 1), (2, 2, 2)], true)[0].clone();
        assert!(out.contains("888"), "{out:?}");
        assert!(out.contains("\x1b[38;2;2;2;2m"));
    }

    #[test]
    fn double_dollar_is_a_literal_dollar() {
        let out = render_art("$$", &[(1, 2, 3)], true)[0].clone();
        assert!(out.contains('$'));
        assert_eq!(out.matches('$').count(), 1);
    }

    #[test]
    fn bare_dollar_stays_literal() {
        assert!(render_art("100$", &[(1, 2, 3)], true)[0].contains('$'));
        assert!(render_art("$ x", &[(1, 2, 3)], true)[0].contains('$'));
    }

    #[test]
    fn out_of_range_marker_keeps_dollar_and_digit() {
        let out = render_art("$9x", &[(1, 2, 3)], true)[0].clone();
        assert!(out.contains("$9"), "{out:?}");
        assert!(out.trim_end_matches(RESET).ends_with('x'), "{out:?}");
    }

    #[test]
    fn color_carries_over_to_the_next_line() {
        // Matches fastfetch: the active colour persists until the next marker.
        let out = render_art("$2aa\nbb", &[(1, 1, 1), (2, 2, 2)], true);
        assert!(out[0].contains("\x1b[38;2;2;2;2m"));
        assert!(out[1].starts_with("\x1b[38;2;2;2;2m"), "{:?}", out[1]);
    }

    #[test]
    fn tabs_expand_to_four_spaces() {
        let out = render_art("a\tb", &[(1, 2, 3)], true)[0].clone();
        assert!(out.contains("a    b"), "{out:?}");
        assert!(!out.contains('\t'));
    }

    #[test]
    fn no_color_drops_markers_entirely() {
        let out = render_art("$1ab$2cd", &[(1, 2, 3), (4, 5, 6)], false)[0].clone();
        assert_eq!(out, "abcd");
    }

    #[test]
    fn no_color_keeps_literal_dollar() {
        assert_eq!(render_art("$$", &[(1, 2, 3)], false)[0], "$");
    }

    #[test]
    fn plain_lines_pass_through() {
        let out = render_art("no markers", &[(1, 2, 3)], false)[0].clone();
        assert_eq!(out, "no markers");
    }

    #[test]
    fn width_ignores_escape_sequences() {
        let logo = Logo::new("$1ab\nabc", &[(1, 2, 3)], true);
        assert_eq!(logo.width(), 3);
    }

    #[test]
    fn width_measures_unicode_columns_with_or_without_colour() {
        for color in [false, true] {
            let logo = Logo::new("$1日本\ne\u{301}\n👩‍💻", &[(1, 2, 3)], color);
            assert_eq!(logo.width(), 4);
            // Palette changes must not split the visible emoji sequence.
            let emoji = Logo::new("👩$1\u{200d}💻", &[(1, 2, 3)], color);
            assert_eq!(emoji.width(), 2);
        }
    }

    #[test]
    fn width_is_zero_for_blank_art() {
        assert_eq!(Logo::new("", &[(1, 2, 3)], true).width(), 0);
        assert_eq!(Logo::new("\n\n", &[(1, 2, 3)], true).width(), 0);
    }

    #[test]
    fn normalizes_case_and_dashes() {
        assert_eq!(normalize("  Fedora "), "fedora");
        assert_eq!(normalize("KDE-Plasma"), "kde_plasma");
    }

    #[test]
    fn finds_by_name_and_alias() {
        assert!(find("fedora").is_some());
        assert!(find("Fedora").is_some());
        assert!(find("FEDORA").is_some());
    }

    #[test]
    fn unknown_name_returns_none() {
        assert!(find("").is_none());
        assert!(find("   ").is_none());
        assert!(find("definitely-not-a-distro-xyz").is_none());
    }

    #[test]
    fn detect_falls_back_to_unknown() {
        assert_eq!(detect(None).names.first(), Some(&"unknown"));
        assert_eq!(detect(Some("no-such-os")).names.first(), Some(&"unknown"));
        assert_ne!(detect(Some("fedora")).names.first(), Some(&"unknown"));
    }

    #[test]
    fn build_resolves_real_art() {
        let raw = find("fedora").unwrap();
        let style = Style::default();
        let logo = build(raw, &style);
        assert!(logo.width() > 10);
        assert!(logo.lines().len() > 5);
        assert!(logo.lines().iter().any(|l| l.contains('\x1b')));
    }

    #[test]
    fn build_without_colour_emits_no_escapes() {
        let style = Style {
            colors: false,
            ..Default::default()
        };
        for raw in LOGOS {
            let logo = build(raw, &style);
            for line in logo.lines() {
                assert!(!line.contains('\x1b'), "{:?}: {line:?}", raw.names);
                assert!(!line.contains('\t'), "{:?}: {line:?}", raw.names);
            }
        }
    }
}
