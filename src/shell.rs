//! Shell integration: the line that activates the configured prompt.
//!
//! The prompt is chosen once, in configuration, and both the install and the
//! activation follow from it. Dotfiles then contain a single line,
//! `eval "$(nt shell-init bash)"`, and changing the prompt is a configuration
//! change rather than an edit to three files.

use anyhow::{Result, bail};

/// The shell code that activates `prompt` in `shell`.
///
/// `powerbash` is a bash library, so asking for it from another shell is an
/// error rather than a silently empty string.
pub fn init(prompt: &str, shell: &str) -> Result<String> {
    // fish has no `eval "$(...)"` idiom; it sources a pipe instead.
    let line = match (prompt, shell) {
        ("starship", "bash" | "zsh") => format!("eval \"$(starship init {shell})\""),
        ("starship", "fish") => "starship init fish | source".to_string(),
        ("oh-my-posh", "bash" | "zsh") => format!("eval \"$(oh-my-posh init {shell})\""),
        ("oh-my-posh", "fish") => "oh-my-posh init fish | source".to_string(),
        ("powerbash", "bash") => {
            // Installed by its Homebrew formula under the prefix's share dir;
            // resolve the prefix at shell start so the line works wherever
            // Homebrew lives.
            "source \"$(brew --prefix)/share/powerbash/powerbash.sh\"".to_string()
        }
        ("powerbash", other) => bail!("powerbash is a bash library and cannot initialise {other}"),
        (p, s) => bail!("no initialisation known for prompt {p:?} in shell {s:?}"),
    };
    Ok(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starship_initialises_every_supported_shell() {
        assert_eq!(
            init("starship", "bash").unwrap(),
            "eval \"$(starship init bash)\""
        );
        assert_eq!(
            init("starship", "zsh").unwrap(),
            "eval \"$(starship init zsh)\""
        );
        assert_eq!(
            init("starship", "fish").unwrap(),
            "starship init fish | source"
        );
    }

    #[test]
    fn oh_my_posh_initialises_every_supported_shell() {
        assert_eq!(
            init("oh-my-posh", "bash").unwrap(),
            "eval \"$(oh-my-posh init bash)\""
        );
        assert_eq!(
            init("oh-my-posh", "fish").unwrap(),
            "oh-my-posh init fish | source"
        );
    }

    #[test]
    fn powerbash_is_bash_only() {
        assert!(init("powerbash", "bash").unwrap().contains("powerbash.sh"));
        let err = init("powerbash", "zsh").unwrap_err();
        assert!(format!("{err:#}").contains("bash"), "got {err:#}");
    }

    #[test]
    fn every_catalog_prompt_initialises_bash() {
        for p in crate::config::PROMPTS {
            assert!(init(p, "bash").is_ok(), "{p}");
        }
    }

    #[test]
    fn an_unknown_combination_is_an_error() {
        assert!(init("p10k", "bash").is_err());
        assert!(init("starship", "tcsh").is_err());
    }
}
