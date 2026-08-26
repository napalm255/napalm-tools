//! Spinner wiring.
//!
//! Drawing happens only when [`super::Ui`] decides a person is watching
//! stderr. Every other case - a pipe, a log, JSON, `--quiet` - gets a
//! disabled `Progress` whose methods do nothing, so callers never branch on
//! the format themselves.

use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

/// How often the spinner advances.
const TICK: Duration = Duration::from_millis(80);

/// Owns the terminal's live region, when there is one.
pub struct Progress {
    multi: Option<MultiProgress>,
}

impl Progress {
    /// A `Progress` that draws when `live`, and otherwise does nothing.
    pub fn new(live: bool) -> Progress {
        if live {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drawing_is_off_unless_live() {
        assert!(Progress::new(false).spinner("x".into()).is_none());
    }

    #[test]
    fn a_pretty_progress_produces_a_spinner_without_panicking() {
        // Exercised for absence of panic; its rendering is not asserted on.
        let p = Progress::new(true);
        if let Some(bar) = p.spinner("working".into()) {
            bar.set_message("still working");
            bar.finish_and_clear();
        }
    }
}
