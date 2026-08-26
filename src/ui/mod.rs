//! Presentation: everything `nt` writes, in one place.
//!
//! Two rules hold this together.
//!
//! **Channel discipline.** Stdout carries the answer - the rendered plan, the
//! notes, any JSON - and nothing else. Stderr carries progress, warnings and
//! errors. That is what makes `nt apply --output json > file` produce a file
//! that parses.
//!
//! **One layer.** Nothing outside this module writes to stdout or stderr, so
//! how `nt` presents itself is a single decision rather than a dozen scattered
//! `println!` calls.

pub mod json;
pub mod progress;
pub mod scan;
pub mod theme;

use std::io::{IsTerminal, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::execute::RunReport;
use theme::Theme;

/// How output is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Spinners and styling, for a terminal.
    Pretty,
    /// One line per event, identical in a pipe or a log.
    Plain,
    /// A single machine-readable document on stdout.
    Json,
}

impl Format {
    /// The format to use when none was requested: decoration only earns its
    /// place when a human is watching, and the answer goes to stdout, so
    /// stdout is what decides. `nt bundles > file` must never put escape
    /// sequences in the file, however lively the terminal on stderr.
    pub fn detect() -> Format {
        if std::io::stdout().is_terminal() {
            Format::Pretty
        } else {
            Format::Plain
        }
    }
}

impl std::str::FromStr for Format {
    type Err = String;

    fn from_str(s: &str) -> Result<Format, String> {
        match s {
            "pretty" => Ok(Format::Pretty),
            "plain" => Ok(Format::Plain),
            "json" => Ok(Format::Json),
            other => Err(format!("unknown output format {other:?}")),
        }
    }
}

/// Text captured instead of written, for tests.
#[derive(Debug, Default)]
pub struct Captured {
    /// What would have gone to stdout.
    pub out: String,
    /// What would have gone to stderr.
    pub err: String,
}

/// Where a [`Ui`] writes.
enum Sink {
    /// The real streams.
    Std,
    /// A buffer, so tests can assert on exact output.
    Capture(Arc<Mutex<Captured>>),
}

/// The single point through which `nt` speaks.
pub struct Ui {
    format: Format,
    theme: Theme,
    /// 0 captures subprocess output; 1 or more streams it through untouched.
    verbosity: u8,
    quiet: bool,
    sink: Sink,
    progress: progress::Progress,
}

impl Ui {
    /// A `Ui` writing to the real streams.
    ///
    /// Progress lives on stderr and is drawn only when stderr is a terminal,
    /// independently of the format: a pretty answer piped to `less` still
    /// gets a spinner, and a JSON answer never does.
    pub fn new(format: Format, verbosity: u8, quiet: bool) -> Ui {
        let live = format != Format::Json && !quiet && std::io::stderr().is_terminal();
        Ui {
            format,
            theme: Theme::for_format(format),
            verbosity,
            quiet,
            sink: Sink::Std,
            progress: progress::Progress::new(live),
        }
    }

    /// A `Ui` capturing everything, with a handle to inspect what it wrote.
    pub fn capturing(format: Format) -> (Ui, Arc<Mutex<Captured>>) {
        Ui::capturing_quiet(format, false)
    }

    /// A capturing `Ui` with an explicit quiet setting.
    pub fn capturing_quiet(format: Format, quiet: bool) -> (Ui, Arc<Mutex<Captured>>) {
        let buf = Arc::new(Mutex::new(Captured::default()));
        let ui = Ui {
            format,
            // Captured output is asserted on, so never decorated.
            theme: Theme::plain(),
            verbosity: 0,
            quiet,
            sink: Sink::Capture(Arc::clone(&buf)),
            progress: progress::Progress::disabled(),
        };
        (ui, buf)
    }

    /// Whether subprocess output should stream through rather than be captured.
    pub fn raw_subprocess_output(&self) -> bool {
        self.verbosity > 0
    }

    /// The active format.
    pub fn format(&self) -> Format {
        self.format
    }

    /// The styles to render human-facing output with.
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Write the answer to stdout, exactly as given.
    ///
    /// Suppressed by `--quiet`, which means silence on success - including
    /// the answer. Asking for quiet and for output at once is contradictory,
    /// and quiet is the more specific request.
    pub fn data(&self, text: &str) {
        if self.quiet {
            return;
        }
        self.write_out(text);
    }

    /// Write an informational line to stderr.
    ///
    /// Suppressed under `--quiet` and in JSON mode, where anything but the
    /// document itself is noise.
    pub fn line(&self, msg: &str) {
        if self.quiet || self.format == Format::Json {
            return;
        }
        self.write_err(&format!("{msg}\n"));
    }

    /// Write a warning to stderr.
    ///
    /// Suppressed by JSON mode, where it would corrupt the document, and by
    /// `--quiet`, where only failures are worth breaking silence for.
    pub fn warn(&self, msg: &str) {
        if self.quiet || self.format == Format::Json {
            return;
        }
        self.write_err(&format!("{} {msg}\n", self.theme.warn.apply_to("warning:")));
    }

    /// Write an error to stderr.
    ///
    /// Never suppressed: a failure must not be silent in any mode.
    pub fn error(&self, msg: &str) {
        self.write_err(&format!("{} {msg}\n", self.theme.bad.apply_to("error:")));
    }

    /// Begin an open-ended activity with no step number - checking, probing,
    /// anything whose duration is unknown and whose only news is that it is
    /// still going.
    pub fn probe(&self, label: &str) -> Probe<'_> {
        let bar = if self.silent_steps() {
            None
        } else {
            self.progress.spinner(label.to_string())
        };
        Probe {
            ui: self,
            label: label.to_string(),
            bar,
            started: std::time::Instant::now(),
        }
    }

    /// Begin a step, returning a handle that reports its outcome.
    pub fn step(&self, index: usize, total: usize, label: &str) -> Step<'_> {
        let label = label.to_string();
        let bar = if self.silent_steps() {
            None
        } else {
            self.progress.spinner(format!("[{index}/{total}] {label}"))
        };
        // Without a spinner there is no live region, so announce the start;
        // with one, the spinner already says what is running.
        if bar.is_none() && !self.silent_steps() {
            self.write_err(&format!("  [{index}/{total}] {label}\n"));
        }
        Step {
            ui: self,
            index,
            total,
            label,
            bar,
        }
    }

    /// Write the end-of-run summary to stderr.
    ///
    /// Under `--quiet` only a failure is reported: silence means success.
    pub fn summary(&self, report: &RunReport) {
        if self.format == Format::Json {
            return;
        }
        let failed = report.steps.iter().filter(|s| !s.success).count();
        if self.quiet {
            if failed > 0 {
                self.write_err(&format!(
                    "{failed} step{} failed\n",
                    if failed == 1 { "" } else { "s" }
                ));
            }
            return;
        }
        let t = &self.theme;

        if !report.steps.is_empty() {
            let mut line = format!(
                "{} step{} in {}",
                report.steps.len(),
                if report.steps.len() == 1 { "" } else { "s" },
                human_duration(report.total)
            );
            if failed > 0 {
                line.push_str(&format!(", {failed} failed"));
                self.write_err(&format!("\n{} {}\n", t.cross(), t.bad.apply_to(line)));
            } else {
                self.write_err(&format!("\n{} {}\n", t.tick(), t.good.apply_to(line)));
            }
        }

        for caveat in &report.findings.caveats {
            self.write_err(&format!(
                "\n{} {}\n",
                t.note_icon(),
                t.heading
                    .apply_to(format!("note from `{}`:", caveat.source))
            ));
            for line in &caveat.lines {
                self.write_err(&format!("  {line}\n"));
            }
        }

        if !report.findings.warnings.is_empty() {
            self.write_err(&format!(
                "\n{} {}\n",
                t.warn_icon(),
                t.heading.apply_to("warnings:")
            ));
            for w in &report.findings.warnings {
                self.write_err(&format!("  {}\n", t.warn.apply_to(w)));
            }
        }
    }

    /// Whether step reporting should be silent.
    fn silent_steps(&self) -> bool {
        self.quiet || self.format == Format::Json
    }

    /// Emit a completed step's line.
    ///
    /// Written straight to stderr rather than through the progress region:
    /// the step's own spinner has already been cleared, and indicatif's
    /// `println` ends its line with a carriage return once no bar is live,
    /// which left the next stdout line glued to this one.
    fn step_line(&self, text: &str) {
        self.write_err(text);
    }

    fn write_out(&self, text: &str) {
        match &self.sink {
            Sink::Std => {
                let mut o = std::io::stdout();
                let _ = o.write_all(text.as_bytes());
                let _ = o.flush();
            }
            Sink::Capture(buf) => buf.lock().unwrap().out.push_str(text),
        }
    }

    fn write_err(&self, text: &str) {
        match &self.sink {
            Sink::Std => {
                let mut e = std::io::stderr();
                let _ = e.write_all(text.as_bytes());
                let _ = e.flush();
            }
            Sink::Capture(buf) => buf.lock().unwrap().err.push_str(text),
        }
    }
}

/// An open-ended activity, shown while it runs and summarised when it ends.
pub struct Probe<'a> {
    ui: &'a Ui,
    label: String,
    bar: Option<indicatif::ProgressBar>,
    started: std::time::Instant,
}

impl Probe<'_> {
    /// Note what the activity is currently doing.
    pub fn detail(&self, what: &str) {
        if let Some(bar) = &self.bar {
            bar.set_message(format!("{}  {}", self.label, what));
        }
    }

    /// Close the activity, replacing it with a one-line result.
    pub fn finish(self, summary: &str) {
        if let Some(bar) = self.bar {
            bar.finish_and_clear();
        }
        if self.ui.silent_steps() {
            return;
        }
        let theme = self.ui.theme();
        self.ui.step_line(&format!(
            "  {} {} {}\n",
            theme.good.apply_to("·"),
            summary,
            theme
                .dim
                .apply_to(format!("({})", human_duration(self.started.elapsed())))
        ));
    }
}

/// A single running step.
pub struct Step<'a> {
    ui: &'a Ui,
    index: usize,
    total: usize,
    label: String,
    bar: Option<indicatif::ProgressBar>,
}

impl Step<'_> {
    /// Update the live detail line with the latest output from the command.
    ///
    /// Only a spinner can show this; without one there is nowhere to put a
    /// line that will be replaced a moment later.
    pub fn detail(&self, line: &str) {
        let Some(bar) = &self.bar else { return };
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        bar.set_message(format!(
            "[{}/{}] {}  {}",
            self.index,
            self.total,
            self.label,
            truncate(line, 60)
        ));
    }

    /// Close the step.
    pub fn finish(self, success: bool, elapsed: Duration) {
        if self.ui.silent_steps() {
            if let Some(bar) = self.bar {
                bar.finish_and_clear();
            }
            return;
        }

        let theme = self.ui.theme();
        let mark = if success { theme.tick() } else { theme.cross() };
        let text = format!(
            "  {} {} {} {}\n",
            theme
                .dim
                .apply_to(format!("[{}/{}]", self.index, self.total)),
            theme.name.apply_to(&self.label),
            mark,
            theme.dim.apply_to(format!("({})", human_duration(elapsed)))
        );

        if let Some(bar) = self.bar {
            bar.finish_and_clear();
        }
        self.ui.step_line(&text);
    }
}

/// Shorten `text` to `max` characters, marking that it was cut.
fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}\u{2026}")
}

/// Render a duration the way a person reads it.
pub fn human_duration(d: Duration) -> String {
    let secs = d.as_secs_f64();
    if secs < 1.0 {
        format!("{}ms", d.as_millis())
    } else if secs < 60.0 {
        format!("{secs:.1}s")
    } else {
        format!("{}m{:02}s", d.as_secs() / 60, d.as_secs() % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn captured(format: Format) -> (Ui, Arc<Mutex<Captured>>) {
        Ui::capturing(format)
    }

    fn out(buf: &Arc<Mutex<Captured>>) -> String {
        buf.lock().unwrap().out.clone()
    }

    fn err(buf: &Arc<Mutex<Captured>>) -> String {
        buf.lock().unwrap().err.clone()
    }

    #[test]
    fn data_goes_to_stdout_verbatim() {
        let (ui, buf) = captured(Format::Plain);

        ui.data("core  on\n");

        assert_eq!(out(&buf), "core  on\n");
        assert!(err(&buf).is_empty(), "data must not touch stderr");
    }

    #[test]
    fn diagnostics_never_reach_stdout() {
        // The rule that keeps `nt ... > file` parseable.
        let (ui, buf) = captured(Format::Plain);

        ui.line("starting");
        ui.warn("something odd");
        ui.error("it broke");

        assert!(out(&buf).is_empty(), "stdout was polluted: {:?}", out(&buf));
        assert!(!err(&buf).is_empty());
    }

    #[test]
    fn warnings_and_errors_are_labelled() {
        let (ui, buf) = captured(Format::Plain);

        ui.warn("something odd");
        ui.error("it broke");

        let e = err(&buf);
        assert!(e.contains("warning: something odd"), "got {e:?}");
        assert!(e.contains("error: it broke"), "got {e:?}");
    }

    #[test]
    fn a_step_reports_its_start_and_outcome_in_plain_mode() {
        let (ui, buf) = captured(Format::Plain);

        let step = ui.step(1, 3, "brew install nmap");
        step.finish(true, Duration::from_millis(1200));

        let e = err(&buf);
        assert!(e.contains("[1/3]"), "got {e:?}");
        assert!(e.contains("brew install nmap"), "got {e:?}");
        assert!(e.contains("1.2s"), "got {e:?}");
    }

    #[test]
    fn a_failed_step_is_marked_as_such() {
        let (ui, buf) = captured(Format::Plain);

        ui.step(2, 3, "brew install broken")
            .finish(false, Duration::from_millis(500));

        let e = err(&buf).to_lowercase();
        assert!(e.contains("fail") || e.contains("✗"), "got {e:?}");
    }

    #[test]
    fn json_mode_emits_no_decoration() {
        // Anything but the document itself would corrupt the output.
        let (ui, buf) = captured(Format::Json);

        ui.line("starting");
        ui.step(1, 1, "brew install nmap")
            .finish(true, Duration::from_secs(1));

        assert!(err(&buf).is_empty(), "got {:?}", err(&buf));
        assert!(out(&buf).is_empty());
    }

    #[test]
    fn json_mode_still_reports_errors() {
        // A failure must never be silent, whatever the format.
        let (ui, buf) = captured(Format::Json);

        ui.error("it broke");

        assert!(err(&buf).contains("it broke"));
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(human_duration(Duration::from_millis(420)), "420ms");
        assert_eq!(human_duration(Duration::from_millis(1200)), "1.2s");
        assert_eq!(human_duration(Duration::from_secs(90)), "1m30s");
    }

    #[test]
    fn raw_subprocess_output_follows_verbosity() {
        assert!(!Ui::new(Format::Plain, 0, false).raw_subprocess_output());
        assert!(Ui::new(Format::Plain, 1, false).raw_subprocess_output());
        assert!(Ui::new(Format::Plain, 2, false).raw_subprocess_output());
    }

    #[test]
    fn formats_parse_from_their_names() {
        use std::str::FromStr;
        assert_eq!(Format::from_str("json").unwrap(), Format::Json);
        assert_eq!(Format::from_str("plain").unwrap(), Format::Plain);
        assert_eq!(Format::from_str("pretty").unwrap(), Format::Pretty);
        assert!(Format::from_str("fancy").is_err());
    }

    #[test]
    fn quiet_suppresses_everything_but_errors() {
        // `-q` means silence on success, including the answer itself.
        let (ui, buf) = Ui::capturing_quiet(Format::Plain, true);

        ui.data("core  on\n");
        ui.line("starting");
        ui.warn("something odd");
        ui.step(1, 1, "brew install nmap")
            .finish(true, Duration::from_secs(1));
        ui.summary(&RunReport::default());

        assert!(out(&buf).is_empty(), "stdout not silent: {:?}", out(&buf));
        assert!(err(&buf).is_empty(), "stderr not silent: {:?}", err(&buf));
    }

    #[test]
    fn quiet_still_reports_errors() {
        // Silence on success; never silence on failure.
        let (ui, buf) = Ui::capturing_quiet(Format::Plain, true);

        ui.error("it broke");

        assert!(err(&buf).contains("it broke"));
    }

    #[test]
    fn quiet_does_not_suppress_a_failed_step_summary() {
        let (ui, buf) = Ui::capturing_quiet(Format::Plain, true);
        let report = RunReport {
            steps: vec![crate::execute::StepOutcome {
                command: "brew install broken".into(),
                duration: Duration::from_secs(1),
                success: false,
            }],
            ..Default::default()
        };

        ui.summary(&report);

        assert!(
            err(&buf).contains("failed"),
            "a failure must survive --quiet, got {:?}",
            err(&buf)
        );
    }
}
