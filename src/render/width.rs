//! ANSI-aware terminal column measurement.
//!
//! Layout maths must never count escape sequences as visible characters,
//! otherwise coloured keys shift the value column to the right.

use unicode_width::UnicodeWidthStr;

/// Returns the terminal column width of a single line, ignoring ANSI escape
/// sequences (CSI `ESC [ ... final-byte`) and OSC sequences. Uses non-CJK
/// widths for ambiguous characters, preserving one-column Nerd Font icons.
pub fn visible_len(s: &str) -> usize {
    if !s.contains('\x1b') {
        return s.width();
    }

    // Measure the whole visible string rather than summing character widths:
    // emoji sequences can span multiple code points (and ANSI style changes).
    let mut plain = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            skip_escape(&mut chars);
            continue;
        }
        plain.push(c);
    }
    plain.width()
}

/// Consumes the remainder of an escape sequence.
///
/// Handles both CSI (`ESC [ ... final`) and OSC (`ESC ] ... BEL | ESC \`),
/// plus two-byte `ESC <char>` forms. Unknown sequences degrade gracefully:
/// at most a single character is swallowed so measurement stays finite.
fn skip_escape<I: Iterator<Item = char>>(chars: &mut std::iter::Peekable<I>) {
    let Some(kind) = chars.peek().copied() else {
        return;
    };
    match kind {
        '[' => {
            chars.next();
            // CSI ends on a byte in the range 0x40..=0x7E.
            while let Some(&c) = chars.peek() {
                chars.next();
                if ('\x40'..='\x7e').contains(&c) {
                    break;
                }
            }
        }
        ']' => {
            chars.next();
            let mut prev = '\0';
            for c in chars.by_ref() {
                if prev == '\x1b' && c == '\\' {
                    break;
                }
                if c == '\x07' {
                    break;
                }
                prev = c;
            }
        }
        _ => {
            chars.next();
        }
    }
}

/// Right-pads `s` with spaces to `width` visible columns.
/// Never truncates: callers decide how to handle overflow.
pub fn pad_right(s: &str, width: usize) -> String {
    let len = visible_len(s);
    let mut out = String::with_capacity(s.len() + width.saturating_sub(len));
    out.push_str(s);
    out.extend(std::iter::repeat_n(' ', width.saturating_sub(len)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_string_length() {
        assert_eq!(visible_len("hello"), 5);
        assert_eq!(visible_len(""), 0);
    }

    #[test]
    fn ignores_csi_sequences() {
        assert_eq!(visible_len("\x1b[1;31mabc\x1b[0m"), 3);
        assert_eq!(visible_len("\x1b[38;2;60;110;180mx\x1b[0m"), 1);
    }

    #[test]
    fn ignores_osc_sequences() {
        assert_eq!(visible_len("\x1b]0;title\x07ok"), 2);
        assert_eq!(visible_len("\x1b]8;;http://x\x1b\\link"), 4);
    }

    #[test]
    fn truncated_escape_does_not_hang() {
        // Unterminated sequences must terminate safely, hiding their contents.
        assert_eq!(visible_len("\x1b[1;3"), 0);
        assert_eq!(visible_len("a\x1b"), 1);
        assert_eq!(visible_len("a\x1b]unfinished"), 1);
    }

    #[test]
    fn pad_right_uses_visible_width() {
        let colored = "\x1b[1mab\x1b[0m";
        assert_eq!(visible_len(&pad_right(colored, 5)), 5);
        assert_eq!(visible_len(&pad_right("abc", 2)), 3);
    }

    #[test]
    fn unicode_uses_terminal_columns_not_code_points() {
        assert_eq!(visible_len("привет"), 6);
        assert_eq!(visible_len("日本"), 4);
        assert_eq!(visible_len("ＡＢ"), 4);
        assert_eq!(visible_len("A日本B"), 6);
    }

    #[test]
    fn combining_marks_do_not_add_columns() {
        assert_eq!(visible_len("e\u{301}"), 1);
        assert_eq!(visible_len("é"), 1);
        assert_eq!(visible_len("\u{301}"), 0);
        assert_eq!(visible_len("\x1b[31me\x1b[0m\u{301}"), 1);
    }

    #[test]
    fn emoji_sequences_use_their_combined_width() {
        for emoji in ["🦀", "👩‍💻", "👨‍👩‍👧‍👦", "👍🏽", "🇯🇵", "❤️", "1️⃣"]
        {
            assert_eq!(visible_len(emoji), 2, "wrong width for {emoji:?}");
        }
        assert_eq!(visible_len("👩\x1b[31m\u{200d}💻\x1b[0m"), 2);
        assert_eq!(visible_len("❤\x1b[31m\u{fe0f}\x1b[0m"), 2);
    }

    #[test]
    fn wide_text_ignores_csi_and_osc_sequences() {
        assert_eq!(visible_len("\x1b[31m日本\x1b[0m"), 4);
        assert_eq!(
            visible_len("\x1b]8;;https://example.org\x07日本\x1b]8;;\x07"),
            4
        );
        assert_eq!(
            visible_len("\x1b]8;;https://example.org\x1b\\日本\x1b]8;;\x1b\\"),
            4
        );
    }

    #[test]
    fn nerd_font_and_ambiguous_characters_stay_single_column() {
        assert_eq!(visible_len(&crate::render::NERD_SEPARATOR.to_string()), 1);
        assert_eq!(visible_len("·Ω"), 2);
    }

    #[test]
    fn padding_uses_unicode_columns_without_truncation() {
        assert_eq!(pad_right("日本", 6), "日本  ");
        assert_eq!(pad_right("e\u{301}", 3), "e\u{301}  ");
        assert_eq!(pad_right("👩‍💻", 4), "👩‍💻  ");
        assert_eq!(pad_right("日本", 2), "日本");
        let colored = "\x1b[31m日本\x1b[0m";
        assert_eq!(pad_right(colored, 6), format!("{colored}  "));
    }
}
