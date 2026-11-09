//! Current shell from ancestry, with the login shell only as a fallback.

fn shell_name(name: &str) -> Option<&'static str> {
    match name.trim_start_matches('-') {
        "bash" => Some("bash"),
        "zsh" => Some("zsh"),
        "fish" => Some("fish"),
        "sh" => Some("sh"),
        "dash" => Some("dash"),
        "ash" => Some("ash"),
        "ksh" | "ksh93" => Some("ksh"),
        "mksh" => Some("mksh"),
        "tcsh" => Some("tcsh"),
        "csh" => Some("csh"),
        "nu" => Some("nu"),
        "xonsh" => Some("xonsh"),
        "elvish" => Some("elvish"),
        "pwsh" => Some("pwsh"),
        _ => None,
    }
}

pub(super) fn detect(parents: &[String], login_shell: Option<&str>) -> Option<String> {
    parents
        .iter()
        .find_map(|p| shell_name(p))
        .or_else(|| shell_name(login_shell?.rsplit('/').next()?))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_shell_wins_over_login_and_intermediaries() {
        let parents = ["sudo", "", "env", "fish", "bash"].map(str::to_string);
        assert_eq!(detect(&parents, Some("/bin/zsh")).as_deref(), Some("fish"));
    }

    #[test]
    fn fallback_and_missing_fixtures() {
        assert_eq!(detect(&[], Some("/usr/bin/zsh")).as_deref(), Some("zsh"));
        assert_eq!(detect(&["-bash".into()], None).as_deref(), Some("bash"));
        for fallback in [None, Some(""), Some("/bin/not-a-shell")] {
            assert_eq!(detect(&["wrapper".into()], fallback), None);
        }
    }
}
