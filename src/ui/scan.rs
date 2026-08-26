//! Finding the few lines worth showing in a manager's output.
//!
//! Package managers emit a great deal of text, nearly all of it noise once a
//! command has succeeded. A little of it matters and currently scrolls past
//! unread: Homebrew's `==> Caveats` blocks, which say things like "activate
//! this in your shell", and deprecation notices that predict tomorrow's
//! failure.
//!
//! Pure: lines in, findings out. No subprocess is needed to test it.

/// The most warnings retained, so a pathological run cannot flood the summary.
const MAX_WARNINGS: usize = 20;
/// The most caveat blocks retained.
const MAX_CAVEATS: usize = 10;

/// A caveat block emitted by a manager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caveat {
    /// The command that produced it, filled in by the caller.
    pub source: String,
    /// The block's lines, with blank leading and trailing lines removed.
    pub lines: Vec<String>,
}

/// What a scan turned up.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Findings {
    /// Caveat blocks, in the order encountered.
    pub caveats: Vec<Caveat>,
    /// Warning and deprecation lines, in the order encountered.
    pub warnings: Vec<String>,
}

impl Findings {
    /// Whether anything was found worth reporting.
    pub fn is_empty(&self) -> bool {
        self.caveats.is_empty() && self.warnings.is_empty()
    }
}

/// Consumes lines of manager output and accumulates [`Findings`].
///
/// A struct rather than a free function because caveats span multiple lines:
/// Homebrew opens a block with `==> Caveats` and continues until the next
/// `==>` heading.
#[derive(Debug, Default)]
pub struct Scanner {
    findings: Findings,
    /// Lines of the caveat block currently being collected, if any.
    current: Option<Vec<String>>,
    /// Whether the previous line was a warning still open to continuation.
    in_warning: bool,
    /// The command being scanned, recorded on each caveat.
    source: String,
}

impl Scanner {
    /// A scanner attributing its findings to `source`.
    pub fn new(source: impl Into<String>) -> Scanner {
        Scanner {
            source: source.into(),
            ..Default::default()
        }
    }

    /// Feed one line of output.
    pub fn line(&mut self, line: &str) {
        // A heading always closes whatever block is open, and `==> Caveats`
        // immediately opens a new one.
        if is_heading(line) {
            self.close_block();
            if is_caveat_heading(line) {
                self.current = Some(Vec::new());
            }
            return;
        }

        // Inside a block, every line belongs to it - including a warning,
        // which would otherwise be reported twice in two different sections.
        if let Some(block) = self.current.as_mut() {
            block.push(line.to_string());
            self.in_warning = false;
            return;
        }

        if is_warning(line) {
            if self.findings.warnings.len() < MAX_WARNINGS {
                self.findings.warnings.push(line.trim().to_string());
                self.in_warning = true;
            }
            return;
        }

        // A warning's substance often sits on the indented lines beneath it -
        // "the following taps are not trusted:" means nothing without them.
        if self.in_warning && is_continuation(line) {
            if let Some(last) = self.findings.warnings.last_mut() {
                last.push('\n');
                last.push_str(line.trim_end());
            }
            return;
        }
        self.in_warning = false;
    }

    /// Finish scanning and take the findings.
    pub fn finish(mut self) -> Findings {
        self.close_block();
        self.findings
    }

    /// Store the block being collected, if it holds anything.
    fn close_block(&mut self) {
        let Some(lines) = self.current.take() else {
            return;
        };
        let trimmed = trim_blank_edges(lines);
        if trimmed.is_empty() || self.findings.caveats.len() >= MAX_CAVEATS {
            return;
        }
        self.findings.caveats.push(Caveat {
            source: self.source.clone(),
            lines: trimmed,
        });
    }
}

/// Whether the line is a manager heading such as `==> Fetching mise`.
fn is_heading(line: &str) -> bool {
    line.trim_start().starts_with("==>")
}

/// Whether the heading opens a caveats block.
fn is_caveat_heading(line: &str) -> bool {
    line.trim_start()
        .trim_start_matches("==>")
        .trim()
        .eq_ignore_ascii_case("caveats")
}

/// Whether the line continues the warning above it: indented and non-blank.
fn is_continuation(line: &str) -> bool {
    !line.trim().is_empty() && line.starts_with(char::is_whitespace)
}

/// Whether the line is worth reporting as a warning.
fn is_warning(line: &str) -> bool {
    let trimmed = line.trim_start();
    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("warning:")
        || lower.starts_with("npm warn")
        || lower.starts_with("error:")
        || lower.contains("deprecated")
}

/// Drop blank lines from both ends of a block.
fn trim_blank_edges(mut lines: Vec<String>) -> Vec<String> {
    while lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(text: &str) -> Findings {
        let mut s = Scanner::new("brew install mise");
        for line in text.lines() {
            s.line(line);
        }
        s.finish()
    }

    // Real output shape, taken from `brew info mise` on the development
    // machine: a Caveats block terminated by the next `==>` heading.
    const BREW_WITH_CAVEATS: &str = "\
==> Fetching mise
==> Pouring mise--2025.1.0.x86_64_linux.bottle.tar.gz
==> Caveats
If you are using fish shell, mise will be activated for you automatically.
==> Analytics
install: 74,450 (30 days)
";

    #[test]
    fn a_caveat_block_is_captured() {
        let f = scan(BREW_WITH_CAVEATS);

        assert_eq!(f.caveats.len(), 1, "got {:?}", f.caveats);
        assert_eq!(
            f.caveats[0].lines,
            vec!["If you are using fish shell, mise will be activated for you automatically."]
        );
    }

    #[test]
    fn the_caveat_heading_itself_is_not_part_of_the_block() {
        let f = scan(BREW_WITH_CAVEATS);

        assert!(
            !f.caveats[0].lines.iter().any(|l| l.contains("Caveats")),
            "got {:?}",
            f.caveats[0].lines
        );
    }

    #[test]
    fn the_next_heading_closes_the_block() {
        let f = scan(BREW_WITH_CAVEATS);

        assert!(
            !f.caveats[0].lines.iter().any(|l| l.contains("Analytics")),
            "the block must stop at the next heading, got {:?}",
            f.caveats[0].lines
        );
    }

    #[test]
    fn a_caveat_block_records_the_command_that_produced_it() {
        let f = scan(BREW_WITH_CAVEATS);

        assert_eq!(f.caveats[0].source, "brew install mise");
    }

    #[test]
    fn a_caveat_block_running_to_the_end_of_output_is_still_captured() {
        // Nothing closes the block; finish() has to flush it.
        let f = scan("==> Caveats\nAdd this to your shell profile.\n");

        assert_eq!(f.caveats.len(), 1);
        assert_eq!(f.caveats[0].lines, vec!["Add this to your shell profile."]);
    }

    #[test]
    fn multiple_caveat_blocks_are_all_captured() {
        let f = scan("==> Caveats\nfirst\n==> Fetching x\n==> Caveats\nsecond\n");

        assert_eq!(f.caveats.len(), 2, "got {:?}", f.caveats);
        assert_eq!(f.caveats[1].lines, vec!["second"]);
    }

    #[test]
    fn an_empty_caveat_block_is_not_recorded() {
        // Nothing useful to show, so it should not clutter the summary.
        let f = scan("==> Caveats\n==> Analytics\n");

        assert!(f.caveats.is_empty(), "got {:?}", f.caveats);
    }

    #[test]
    fn brew_warnings_are_collected() {
        let f = scan("Warning: mise 2025.1.0 is already installed and up-to-date.\n");

        assert_eq!(f.warnings.len(), 1);
        assert!(f.warnings[0].contains("already installed"));
    }

    #[test]
    fn npm_warnings_are_collected() {
        let f = scan("npm warn deprecated inflight@1.0.6: This module is not supported\n");

        assert_eq!(f.warnings.len(), 1, "got {:?}", f.warnings);
    }

    #[test]
    fn deprecation_notices_are_collected() {
        let f = scan("formula xyz has been deprecated in favour of abc\n");

        assert_eq!(f.warnings.len(), 1, "got {:?}", f.warnings);
    }

    #[test]
    fn ordinary_progress_output_is_ignored() {
        let f = scan("==> Fetching mise\n==> Pouring mise.bottle.tar.gz\n=> done\n");

        assert!(f.is_empty(), "got {f:?}");
    }

    #[test]
    fn empty_output_yields_nothing() {
        assert!(scan("").is_empty());
    }

    #[test]
    fn a_warning_inside_a_caveat_block_stays_with_the_caveat() {
        // Otherwise the same text is reported twice, in two sections.
        let f = scan("==> Caveats\nWarning: remember to restart your shell\n");

        assert_eq!(f.caveats.len(), 1);
        assert!(f.warnings.is_empty(), "got {:?}", f.warnings);
    }

    #[test]
    fn warnings_are_capped() {
        let many: String = (0..100).map(|i| format!("Warning: number {i}\n")).collect();

        let f = scan(&many);

        assert_eq!(f.warnings.len(), MAX_WARNINGS);
    }

    #[test]
    fn caveat_blocks_are_capped() {
        let many: String = (0..50)
            .map(|i| format!("==> Caveats\nblock {i}\n"))
            .collect();

        let f = scan(&many);

        assert_eq!(f.caveats.len(), MAX_CAVEATS);
    }

    #[test]
    fn blank_lines_around_a_caveat_block_are_trimmed() {
        let f = scan("==> Caveats\n\n  indented note\n\n==> Analytics\n");

        assert_eq!(f.caveats[0].lines, vec!["  indented note"]);
    }

    #[test]
    fn a_warning_absorbs_its_indented_continuation() {
        // Real brew output: the warning's substance is on the lines beneath it,
        // so reporting only the first line throws away the point.
        let f = scan("Warning: The following taps are not trusted:\n  someone/tap\n  other/tap\n");

        assert_eq!(f.warnings.len(), 1, "got {:?}", f.warnings);
        assert!(
            f.warnings[0].contains("someone/tap"),
            "continuation lost: {:?}",
            f.warnings[0]
        );
        assert!(
            f.warnings[0].contains("other/tap"),
            "got {:?}",
            f.warnings[0]
        );
    }

    #[test]
    fn a_warning_continuation_stops_at_an_unindented_line() {
        let f = scan("Warning: something\n  detail\nunrelated output\n");

        assert_eq!(f.warnings.len(), 1);
        assert!(
            !f.warnings[0].contains("unrelated"),
            "got {:?}",
            f.warnings[0]
        );
    }

    #[test]
    fn a_warning_continuation_stops_at_a_blank_line() {
        let f = scan("Warning: something\n  detail\n\n  later unrelated indent\n");

        assert!(!f.warnings[0].contains("later"), "got {:?}", f.warnings[0]);
    }

    #[test]
    fn two_warnings_in_a_row_stay_separate() {
        let f = scan("Warning: first\nWarning: second\n");

        assert_eq!(f.warnings.len(), 2, "got {:?}", f.warnings);
    }
}
