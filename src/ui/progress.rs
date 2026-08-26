//! Spinner wiring.
//!
//! Only [`Format::Pretty`] draws anything. Every other mode - a pipe, a log,
//! JSON, `--quiet` - gets a disabled `Progress` whose methods do nothing, so
//! callers never branch on the format themselves.

use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

use super::Format;

/// How often the spinner advances.
const TICK: Duration = Duration::from_millis(80);

/// Owns the terminal's live region, when there is one.
pub struct Progress {
    multi: Option<MultiProgress>,
}

impl Progress {
    /// A `Progress` that draws only when a person is watching.
    pub fn new(format: Format, quiet: bool) -> Progress {
        if format == Format::Pretty && !quiet {
            Progress {
                multi: Some(MultiProgress::new()),
            }
        } else {
            Progress::disabled()
        }
    }

    /// A `Progress` that draws nothing.
    pub fn disabled() -> Progress {
        Progress { multi: None }
    }

    /// Start a spinner, or return `None` when drawing is disabled.
    pub fn spinner(&self, message: String) -> Option<ProgressBar> {
        let multi = self.multi.as_ref()?;
        let bar = multi.add(ProgressBar::new_spinner());
        bar.set_style(
            ProgressStyle::with_template("  {spinner:.cyan} {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        bar.set_message(message);
        bar.enable_steady_tick(TICK);
        Some(bar)
    }

    /// Print a line above the live region, so it is not overwritten.
    pub fn println(&self, line: &str) -> bool {
        match &self.multi {
            Some(multi) => multi.println(line).is_ok(),
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drawing_is_off_unless_the_format_is_pretty() {
        assert!(
            Progress::new(Format::Plain, false)
                .spinner("x".into())
                .is_none()
        );
        assert!(
            Progress::new(Format::Json, false)
                .spinner("x".into())
                .is_none()
        );
    }

    #[test]
    fn quiet_disables_drawing_even_in_a_terminal() {
        assert!(
            Progress::new(Format::Pretty, true)
                .spinner("x".into())
                .is_none()
        );
    }

    #[test]
    fn a_disabled_progress_swallows_output() {
        assert!(!Progress::disabled().println("nothing"));
    }

    #[test]
    fn a_pretty_progress_produces_a_spinner_without_panicking() {
        // Exercised for absence of panic; its rendering is not asserted on.
        let p = Progress::new(Format::Pretty, false);
        if let Some(bar) = p.spinner("working".into()) {
            bar.set_message("still working");
            bar.finish_and_clear();
        }
    }
}
