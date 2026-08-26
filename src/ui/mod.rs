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

use std::io::{IsTerminal, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::execute::RunReport;

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
    /// place when a human is watching.
    pub fn detect() -> Format {
        if std::io::stderr().is_terminal() {
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
    /// 0 captures subprocess output; 1 or more streams it through untouched.
    verbosity: u8,
    quiet: bool,
    sink: Sink,
    progress: progress::Progress,
}

impl Ui {
    /// A `Ui` writing to the real streams.
    pub fn new(format: Format, verbosity: u8, quiet: bool) -> Ui {
        Ui {
            format,
            verbosity,
            quiet,
            sink: Sink::Std,
            progress: progress::Progress::new(format, quiet),
        }
    }

    /// A `Ui` capturing everything, with a handle to inspect what it wrote.
    pub fn capturing(format: Format) -> (Ui, Arc<Mutex<Captured>>) {
        let buf = Arc::new(Mutex::new(Captured::default()));
        let ui = Ui {
            format,
            verbosity: 0,
            quiet: false,
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

    /// Write the answer to stdout, exactly as given.
    pub fn data(&self, text: &str) {
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

    /// Write a warning to stderr. Suppressed only by JSON mode.
    pub fn warn(&self, msg: &str) {
        if self.format == Format::Json {
            return;
        }
        self.write_err(&format!("warning: {msg}\n"));
    }

    /// Write an error to stderr.
    ///
    /// Never suppressed: a failure must not be silent in any mode.
    pub fn error(&self, msg: &str) {
        self.write_err(&format!("error: {msg}\n"));
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
    pub fn summary(&self, report: &RunReport) {
        if self.quiet || self.format == Format::Json {
            return;
        }

        if !report.steps.is_empty() {
            let failed = report.steps.iter().filter(|s| !s.success).count();
            let mut line = format!(
                "\n{} step{} in {}",
                report.steps.len(),
                if report.steps.len() == 1 { "" } else { "s" },
                human_duration(report.total)
            );
            if failed > 0 {
                line.push_str(&format!(", {failed} failed"));
            }
            self.write_err(&format!("{line}\n"));
        }

        for caveat in &report.findings.caveats {
            self.write_err(&format!("\nnote from `{}`:\n", caveat.source));
            for line in &caveat.lines {
                self.write_err(&format!("  {line}\n"));
            }
        }

        if !report.findings.warnings.is_empty() {
            self.write_err("\nwarnings:\n");
            for w in &report.findings.warnings {
                self.write_err(&format!("  {w}\n"));
            }
        }
    }

    /// Whether step reporting should be silent.
    fn silent_steps(&self) -> bool {
        self.quiet || self.format == Format::Json
    }

    /// Emit a line without disturbing a live spinner region.
    fn step_line(&self, text: &str) {
        if !self.progress.println(text.trim_end()) {
            self.write_err(text);
        }
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

        let mark = if success { "ok" } else { "FAILED" };
        let text = format!(
            "  [{}/{}] {} {} ({})\n",
            self.index,
            self.total,
            self.label,
            mark,
            human_duration(elapsed)
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
}
