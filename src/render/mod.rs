//! Two-column layout: ASCII logo on the left, `Key: value` list on the right.

pub mod color;
pub mod width;

use crate::logo::Logo;
use color::Style;
use width::{pad_right, visible_len};

/// One rendered line of system information.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    pub value: String,
}

/// Separator drawn between key and value. ASCII-safe by default; the nerd font
/// separator can be opted into with `--nerd-separator`.
pub const SEPARATOR: char = ':';
pub const NERD_SEPARATOR: char = '\u{f420}';

#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub separator: char,
    /// Minimum horizontal gap between logo and text columns.
    pub gap: usize,
    /// Padding between the key column and its separator.
    pub key_padding: usize,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            separator: SEPARATOR,
            gap: 3,
            key_padding: 1,
        }
    }
}

impl Layout {
    /// Renders the full output block, trailing newline included.
    ///
    /// `term_width` is used to drop the logo when the two columns would not fit;
    /// pass `None` when the width is unknown (piped output).
    pub fn render(
        &self,
        logo: Option<&Logo>,
        entries: &[Entry],
        style: &Style,
        term_width: Option<usize>,
    ) -> String {
        let logo_width = logo.map(|l| l.width()).unwrap_or(0);
        let key_width = entries
            .iter()
            .map(|e| visible_len(&e.key))
            .max()
            .unwrap_or(0);

        let text_width = key_width
            + self.key_padding
            + visible_len(&self.separator.to_string())
            + 1
            + entries
                .iter()
                .map(|e| visible_len(&e.value))
                .max()
                .unwrap_or(0);

        // Drop the logo rather than wrap the output mid-line.
        let show_logo = logo.is_some()
            && term_width
                .map(|w| logo_width + self.gap + text_width <= w)
                .unwrap_or(true);

        // `$N` markers were already resolved by `Logo::new`; reuse as is.
        let logo_lines: &[String] = match (show_logo, logo) {
            (true, Some(l)) => l.lines(),
            _ => &[],
        };
        let logo_width = if show_logo { logo_width } else { 0 };

        let rows = entries.len().max(logo_lines.len());
        let mut out = String::new();

        for i in 0..rows {
            let mut line = String::new();

            if logo_width > 0 {
                if i < logo_lines.len() {
                    line.push_str(&pad_right(&logo_lines[i], logo_width));
                } else {
                    line.push_str(&pad_right("", logo_width));
                }
                line.push_str(&" ".repeat(self.gap));
            }

            if let Some(entry) = entries.get(i) {
                let key = pad_right(&entry.key, key_width);
                line.push_str(&style.paint_bold(&key, style.key));
                line.push_str(&" ".repeat(self.key_padding));
                let sep = self.separator.to_string();
                let sep = if style.colors {
                    style.paint(&sep, style.key)
                } else {
                    sep
                };
                line.push_str(&sep);
                line.push(' ');
                line.push_str(&style.paint(&entry.value, style.value));
            }

            out.push_str(line.trim_end());
            out.push('\n');
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logo::Logo;

    fn logo(lines: &[&str]) -> Logo {
        Logo::new(&lines.join("\n"), &[(1, 2, 3)], false)
    }

    fn entry(key: &str, value: &str) -> Entry {
        Entry {
            key: key.into(),
            value: value.into(),
        }
    }

    #[test]
    fn aligns_keys_across_entries() {
        let style = Style {
            colors: false,
            ..Default::default()
        };
        let l = Layout::default();
        let out = l.render(
            None,
            &[entry("OS", "Fedora"), entry("Kernel", "6.x")],
            &style,
            None,
        );
        let lines: Vec<&str> = out.lines().collect();
        // The separator sits at key_width + key_padding for every row.
        let expected = "Kernel".len() + l.key_padding;
        assert_eq!(lines[0].find(':'), Some(expected));
        assert_eq!(lines[1].find(':'), Some(expected));
        // Short keys are padded, not shifted left.
        assert!(lines[0].starts_with("OS    "));
    }

    #[test]
    fn aligns_wide_combining_and_emoji_keys() {
        let entries = [
            entry("日本", "A"),
            entry("e\u{301}", "B"),
            entry("👩‍💻", "C"),
            entry("OS", "D"),
        ];
        for colors in [false, true] {
            let style = Style {
                colors,
                ..Default::default()
            };
            let out = Layout::default().render(None, &entries, &style, None);
            if !colors {
                assert_eq!(out, "日本 : A\ne\u{301}    : B\n👩‍💻   : C\nOS   : D\n");
            }
            for line in out.lines() {
                let (prefix, _) = line.split_once(':').unwrap();
                assert_eq!(visible_len(prefix), 5, "misaligned: {line:?}");
            }
        }
    }

    #[test]
    fn pads_wide_logo_lines_to_the_same_text_column() {
        let style = Style {
            colors: false,
            ..Default::default()
        };
        let art = logo(&["日本", "X"]);
        assert_eq!(art.width(), 4);
        let out = Layout::default().render(
            Some(&art),
            &[entry("日本", "A"), entry("OS", "B")],
            &style,
            None,
        );
        assert_eq!(out, "日本   日本 : A\nX      OS   : B\n");
    }

    #[test]
    fn every_logo_aligns_its_text_column_with_or_without_colour() {
        let style = Style {
            colors: false,
            ..Default::default()
        };
        let layout = Layout::default();
        for raw in crate::logo::LOGOS {
            for color in [false, true] {
                let art = Logo::new(raw.art, raw.colors, color);
                let entries = vec![entry("OS", "test"); art.lines().len()];
                let out = layout.render(Some(&art), &entries, &style, None);
                let text_column = if art.width() == 0 {
                    0
                } else {
                    art.width() + layout.gap
                };
                // Out-of-palette markers can legitimately make colored and
                // plain art differ; check each actual rendering independently.
                for line in out.lines() {
                    let prefix = line.strip_suffix("OS : test").unwrap();
                    assert_eq!(
                        visible_len(prefix),
                        text_column,
                        "{:?}: {line:?}",
                        raw.names
                    );
                }
            }
        }
    }

    #[test]
    fn unicode_width_controls_logo_visibility_at_the_exact_boundary() {
        let art = logo(&["日本"]);
        for colors in [false, true] {
            let style = Style {
                colors,
                ..Default::default()
            };
            // Logo (4) + gap (3) + key (2) + padding/separator/space (3),
            // then the value's display width, not its code point count.
            for (value, columns) in [("日本", 16), ("e\u{301}", 13), ("👩‍💻", 14)] {
                let entries = [entry("OS", value)];
                let layout = Layout::default();
                let fits = layout.render(Some(&art), &entries, &style, Some(columns));
                assert!(fits.starts_with("日本"), "logo should fit: {fits:?}");
                assert_eq!(visible_len(fits.trim_end_matches('\n')), columns);
                let narrow = layout.render(Some(&art), &entries, &style, Some(columns - 1));
                assert!(
                    !narrow.starts_with("日本"),
                    "logo should be hidden: {narrow:?}"
                );
            }
        }
    }

    #[test]
    fn plain_output_has_no_escapes() {
        let style = Style {
            colors: false,
            ..Default::default()
        };
        let out = Layout::default().render(None, &[entry("OS", "Fedora")], &style, None);
        assert!(!out.contains('\x1b'));
    }

    #[test]
    fn colored_output_is_ansi_aware_when_aligned() {
        let style = Style::default();
        let out = Layout::default().render(
            None,
            &[entry("OS", "Fedora"), entry("Kernel", "6.x")],
            &style,
            None,
        );
        // Widths measured from visible chars must still line up.
        let cols: Vec<usize> = out
            .lines()
            .map(|l| l.find(':').expect("separator present"))
            .collect();
        assert_eq!(cols[0], cols[1]);
    }

    #[test]
    fn drops_logo_when_terminal_too_narrow() {
        let style = Style {
            colors: false,
            ..Default::default()
        };
        let art = logo(&["XXXXXXXXXXXXXXXX", "YYYY"]);
        let out = Layout::default().render(
            Some(&art),
            &[entry("OS", "Fedora Linux 44")],
            &style,
            Some(30),
        );
        assert!(!out.starts_with("X"));
        assert!(out.contains("OS"));
    }

    #[test]
    fn keeps_logo_when_it_fits() {
        let style = Style {
            colors: false,
            ..Default::default()
        };
        let art = logo(&["AB", "CD"]);
        let out = Layout::default().render(Some(&art), &[entry("OS", "Fedora")], &style, Some(120));
        assert!(out.starts_with("AB"));
    }

    #[test]
    fn nerd_separator_counts_as_one_terminal_column() {
        let style = Style {
            colors: false,
            ..Default::default()
        };
        let art = logo(&["AB"]);
        let layout = Layout {
            separator: NERD_SEPARATOR,
            ..Default::default()
        };
        // 2 logo columns + 3 gap + 2 key + 1 padding + 1 separator
        // + 1 space + 1 value = 11 terminal columns, not 13 UTF-8 bytes.
        let out = layout.render(Some(&art), &[entry("OS", "A")], &style, Some(11));
        assert!(out.starts_with("AB"), "logo dropped despite fitting: {out}");
    }

    #[test]
    fn pads_logo_when_it_is_shorter_than_the_text() {
        let style = Style {
            colors: false,
            ..Default::default()
        };
        let art = logo(&["AB"]);
        let out = Layout::default().render(
            Some(&art),
            &[entry("OS", "A"), entry("Kernel", "B"), entry("Uptime", "C")],
            &style,
            Some(120),
        );
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3);
        // Rows past the logo still start at the text column (logo width + gap).
        let text_col = art.width() + Layout::default().gap;
        assert!(lines[1].starts_with(&" ".repeat(text_col)));
        assert!(lines[1].trim_start().starts_with("Kernel"));
    }

    #[test]
    fn empty_entries_still_render_logo() {
        let style = Style {
            colors: false,
            ..Default::default()
        };
        let art = logo(&["AB", "CD"]);
        let out = Layout::default().render(Some(&art), &[], &style, Some(120));
        assert_eq!(out.lines().count(), 2);
    }
}
