//! How the pipeline talks to the person running it: status lines, download progress and yes/no questions.
//! Everything goes to stderr so stdout stays the report (or `--json`).

use std::cell::Cell;
use std::io::{BufRead, IsTerminal, Write};

pub trait Ui {
    fn info(&self, msg: &str);
    /// A pipeline step starting: short `stage` key, rough overall `fraction` done, and a status line.
    fn step(&self, _stage: &str, _fraction: f64, msg: &str) {
        self.info(msg);
    }
    /// Ask a yes/no question; `false` when nobody can answer.
    fn confirm(&self, question: &str) -> bool;
    fn progress(&self, label: &str, done: u64, total: u64);
}

pub struct ConsoleUi {
    /// Ask on stdin (only honoured when stdin is a terminal).
    pub interactive: bool,
    pub quiet: bool,
    last_pct: Cell<u64>,
}

impl ConsoleUi {
    pub fn new(interactive: bool, quiet: bool) -> Self {
        Self { interactive: interactive && std::io::stdin().is_terminal(), quiet, last_pct: Cell::new(u64::MAX) }
    }
}

impl Ui for ConsoleUi {
    fn info(&self, msg: &str) {
        if !self.quiet {
            eprintln!("{msg}");
        }
    }

    fn confirm(&self, question: &str) -> bool {
        if !self.interactive {
            return false;
        }
        eprint!("{question} [y/N] ");
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line).is_ok() && matches!(line.trim(), "y" | "Y" | "yes" | "Yes")
    }

    fn progress(&self, label: &str, done: u64, total: u64) {
        if self.quiet || total == 0 {
            return;
        }
        let pct = done * 100 / total;
        if pct == self.last_pct.replace(pct) {
            return;
        }
        let end = if done >= total { "\n" } else { "" };
        eprint!("\r  {label}: {pct:>3}% of {:.2} GB{end}", total as f64 / 1e9);
        let _ = std::io::stderr().flush();
    }
}

/// Silent and never agrees; for tests and library callers that pass `yes`.
pub struct QuietUi;

impl Ui for QuietUi {
    fn info(&self, _: &str) {}
    fn confirm(&self, _: &str) -> bool {
        false
    }
    fn progress(&self, _: &str, _: u64, _: u64) {}
}
