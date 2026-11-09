mod cli;
mod fetch;
mod logo;
mod render;

use clap::Parser;
use cli::Cli;
use fetch::Context;
use render::color::Style;

/// Escapes a string for JSON output.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn main() {
    let cli = Cli::parse();

    if cli.logo_list {
        for l in logo::LOGOS {
            let aliases: Vec<&str> = l.names.iter().skip(1).copied().collect();
            if aliases.is_empty() {
                println!("{}", l.names[0]);
            } else {
                println!("{}\t{}", l.names[0], aliases.join(" "));
            }
        }
        return;
    }

    let colors = cli.colors_enabled();

    let modules = cli.selected_modules();
    let ctx = match cli.cpu_usage_interval {
        Some(ms) => {
            Context::collect_with_cpu_interval(&modules, std::time::Duration::from_millis(ms))
        }
        None => Context::collect(&modules),
    };
    let entries = ctx.entries(&modules);

    // Scripted output: no logo, no colour, one field per line.
    if cli.pipe {
        let sep = cli.layout().separator;
        for e in &entries {
            println!("{}{sep} {}", e.key, e.value);
        }
        return;
    }

    if cli.json {
        let fields = entries
            .iter()
            .map(|e| {
                format!(
                    "\"{}\":\"{}\"",
                    json_escape(&e.key.to_lowercase().replace(' ', "_")),
                    json_escape(&e.value)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        println!("{{{fields}}}");
        return;
    }

    // Resolve the artwork first: its palette colours the key column, the way
    // fastfetch defaults `.colorKeys` to the logo's first colour.
    let raw = match (cli.logo_off, cli.logo.as_deref().or_else(|| ctx.os_id())) {
        (true, _) | (_, Some("off")) => None,
        (_, Some(id)) => Some(logo::find(id).unwrap_or_else(logo::unknown)),
        (false, None) => Some(logo::detect(None)),
    };

    let style = Style {
        colors,
        key: raw
            .and_then(|l| l.colors.first().copied())
            .map(|(r, g, b)| owo_colors::Rgb(r, g, b))
            .unwrap_or(Style::default().key),
        ..Style::default()
    };

    let logo = raw.map(|l| logo::build(l, &style));

    print!(
        "{}",
        cli.layout()
            .render(logo.as_ref(), &entries, &style, cli.term_width())
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_quotes_and_backslashes() {
        assert_eq!(json_escape(r#"a"b\c"#), r#"a\"b\\c"#);
    }

    #[test]
    fn escapes_control_characters() {
        assert_eq!(json_escape("a\nb"), "a\\nb");
        assert_eq!(json_escape("a\tb"), "a\\tb");
        assert_eq!(json_escape("\u{1}"), "\\u0001");
    }

    #[test]
    fn leaves_ordinary_text_untouched() {
        assert_eq!(json_escape("Fedora Linux 44"), "Fedora Linux 44");
    }

    #[test]
    fn escapes_cyrillic_unchanged() {
        assert_eq!(json_escape("привет"), "привет");
    }
}
