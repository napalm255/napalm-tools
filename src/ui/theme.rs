//! Colours and glyphs for human-facing output.
//!
//! Styling is decided once, here, rather than at each call site, so that
//! `--output plain` and a pipe produce byte-identical text and only a terminal
//! gets decoration. Emoji follow the same rule and additionally require a
//! UTF-8 terminal; without one every icon falls back to a word or a plain
//! ASCII mark, so the layout still reads.

use console::Style;

use super::Format;

/// The styles used across `nt`'s output.
///
/// A plain theme holds styles that emit nothing, so callers format the same way
/// either way and never branch on whether colour is wanted.
#[derive(Debug, Clone)]
pub struct Theme {
    /// A step or package that succeeded, or an action to be taken.
    pub good: Style,
    /// Something that failed.
    pub bad: Style,
    /// Something worth attention that is not a failure.
    pub warn: Style,
    /// Secondary detail: durations, counts, reasons.
    pub dim: Style,
    /// Section headings.
    pub heading: Style,
    /// A package or command name.
    pub name: Style,
    /// A manager or provider name.
    pub manager: Style,
    /// Whether this theme actually emits escapes.
    coloured: bool,
    /// Whether emoji may be used.
    emoji: bool,
}

impl Theme {
    /// A theme that emits no escape sequences at all.
    pub fn plain() -> Theme {
        Theme {
            good: Style::new(),
            bad: Style::new(),
            warn: Style::new(),
            dim: Style::new(),
            heading: Style::new(),
            name: Style::new(),
            manager: Style::new(),
            coloured: false,
            emoji: false,
        }
    }

    /// A colourful theme.
    pub fn coloured() -> Theme {
        Theme {
            good: Style::new().green(),
            bad: Style::new().red().bold(),
            warn: Style::new().yellow(),
            dim: Style::new().dim(),
            heading: Style::new().bold().magenta(),
            name: Style::new().cyan(),
            manager: Style::new().blue(),
            coloured: true,
            emoji: console::Term::stdout().features().wants_emoji(),
        }
    }

    /// The theme for a format, honouring the terminal and `NO_COLOR`.
    ///
    /// Only `Pretty` is ever decorated: `plain` and `json` are asked for
    /// precisely when the output is going somewhere that should not contain
    /// escapes.
    pub fn for_format(format: Format) -> Theme {
        if format == Format::Pretty && console::colors_enabled() {
            Theme::coloured()
        } else {
            Theme::plain()
        }
    }

    /// Whether this theme emits escapes.
    pub fn is_coloured(&self) -> bool {
        self.coloured
    }

    /// An emoji, or its plain stand-in.
    fn icon(&self, emoji: &'static str, plain: &'static str) -> String {
        if self.emoji {
            emoji.to_string()
        } else {
            plain.to_string()
        }
    }

    /// The marker for a completed step.
    pub fn tick(&self) -> String {
        self.good
            .apply_to(if self.coloured { "✓" } else { "ok" })
            .to_string()
    }

    /// The marker for a failed step.
    pub fn cross(&self) -> String {
        self.bad
            .apply_to(if self.coloured { "✗" } else { "FAILED" })
            .to_string()
    }

    /// Heading icon for the catalog.
    pub fn bundle_icon(&self) -> String {
        self.icon("📦", "")
    }

    /// Heading icon for things that will be installed.
    pub fn install_icon(&self) -> String {
        self.icon("⬇️ ", "")
    }

    /// Heading icon for things already present.
    pub fn satisfied_icon(&self) -> String {
        self.icon("✅", "")
    }

    /// Heading icon for things that cannot be provisioned.
    pub fn warn_icon(&self) -> String {
        self.icon("⚠️ ", "")
    }

    /// Heading icon for things skipped.
    pub fn skip_icon(&self) -> String {
        self.icon("⏭️ ", "")
    }

    /// Heading icon for a note from a manager.
    pub fn note_icon(&self) -> String {
        self.icon("📝", "")
    }

    /// Heading icon for the bootstrap phase.
    pub fn bootstrap_icon(&self) -> String {
        self.icon("🧰", "")
    }

    /// Heading icon for the platform line.
    pub fn platform_icon(&self) -> String {
        self.icon("🖥️ ", "")
    }

    /// The marker for a package that is present.
    pub fn present_mark(&self) -> String {
        self.good
            .apply_to(if self.coloured { "●" } else { "+" })
            .to_string()
    }

    /// The marker for a package that is missing.
    pub fn missing_mark(&self) -> String {
        self.warn
            .apply_to(if self.coloured { "○" } else { "-" })
            .to_string()
    }

    /// The marker for a package that cannot be provisioned.
    pub fn unavailable_mark(&self) -> String {
        self.bad
            .apply_to(if self.coloured { "✗" } else { "!" })
            .to_string()
    }

    /// A heading line: icon (if any), then bold text.
    pub fn heading_line(&self, icon: &str, text: &str) -> String {
        if icon.is_empty() {
            self.heading.apply_to(text).to_string()
        } else {
            format!("{icon} {}", self.heading.apply_to(text))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_escapes(s: &str) -> bool {
        s.contains('\u{1b}')
    }

    #[test]
    fn a_plain_theme_emits_no_escape_sequences_or_emoji() {
        let t = Theme::plain();

        for rendered in [
            t.good.apply_to("x").to_string(),
            t.bad.apply_to("x").to_string(),
            t.warn.apply_to("x").to_string(),
            t.dim.apply_to("x").to_string(),
            t.heading.apply_to("x").to_string(),
            t.name.apply_to("x").to_string(),
            t.manager.apply_to("x").to_string(),
            t.tick(),
            t.cross(),
            t.bundle_icon(),
            t.install_icon(),
            t.satisfied_icon(),
            t.warn_icon(),
            t.skip_icon(),
            t.note_icon(),
            t.bootstrap_icon(),
            t.present_mark(),
            t.missing_mark(),
            t.unavailable_mark(),
            t.heading_line(&t.bundle_icon(), "Bundles"),
        ] {
            assert!(!has_escapes(&rendered), "escapes in {rendered:?}");
            assert!(rendered.is_ascii(), "non-ascii in {rendered:?}");
        }
    }

    #[test]
    fn a_plain_theme_spells_out_its_markers() {
        // Without colour a glyph carries no meaning, so use words.
        let t = Theme::plain();

        assert_eq!(t.tick(), "ok");
        assert_eq!(t.cross(), "FAILED");
        assert_eq!(t.present_mark(), "+");
        assert_eq!(t.missing_mark(), "-");
        assert_eq!(t.unavailable_mark(), "!");
    }

    #[test]
    fn a_plain_heading_has_no_leading_space_without_an_icon() {
        let t = Theme::plain();

        assert_eq!(t.heading_line(&t.bundle_icon(), "Bundles"), "Bundles");
    }

    #[test]
    fn plain_and_json_formats_are_never_decorated() {
        assert!(!Theme::for_format(Format::Plain).is_coloured());
        assert!(!Theme::for_format(Format::Json).is_coloured());
    }

    #[test]
    fn a_plain_theme_still_renders_the_text_it_was_given() {
        let t = Theme::plain();

        assert_eq!(t.name.apply_to("ripgrep").to_string(), "ripgrep");
        assert_eq!(t.dim.apply_to("1.2s").to_string(), "1.2s");
    }
}
