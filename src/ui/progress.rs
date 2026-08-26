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
    fn a_live_progress_produces_a_ticking_spinner_carrying_the_message() {
        let p = Progress::new(true);

        let bar = p.spinner("working".into()).expect("live progress draws");

        assert_eq!(bar.message(), "working");
        assert!(!bar.is_finished());
        bar.set_message("still working");
        assert_eq!(bar.message(), "still working");
        bar.finish_and_clear();
        assert!(bar.is_finished());
    }

    #[test]
    fn a_disabled_progress_never_draws_even_when_asked_twice() {
        let p = Progress::disabled();

        assert!(p.spinner("a".into()).is_none());
        assert!(p.spinner("b".into()).is_none());
    }
}
