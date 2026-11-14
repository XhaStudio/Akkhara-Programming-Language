//! The `akk run` project header: `Compiling`, `Building`, `Running`.
//!
//! The words describe real work in this interpreter, not a code-generation
//! step: "Compiling" is the entry file being read, lexed and parsed;
//! "Building" is the static analysis pass (`library_warnings`, which also
//! parses every script a program brings in, plus the `use`/dependency
//! resolution `cmd_run` already performs); "Running" is the interpreter
//! executing the parsed program.
//!
//! The bar is a **phase indicator printed once**, after parsing has succeeded
//! and the static phases are over -- it is not a live measure of work in
//! progress, and there is no per-step redraw and no timer. A future `akk
//! build` can reuse `Reporter` unchanged.

use std::io::IsTerminal;

/// What the header prints: `[package] name`, `[package] version`, and the
/// project root, already rendered as a display string.
pub struct ProjectHeader {
    pub name: String,
    pub version: String,
    pub root: String,
}

/// Steps the phases cover: entry read, lex+parse, analysis+resolution.
const STEPS: usize = 3;
/// Width of the rendered bar, identical in both renderings.
const WIDTH: usize = 10;

/// Writes the phase lines to stderr. With `header == None` every method is a
/// no-op, so `akk file.akk`, `akk run <file.akk>` and `akk report` are
/// byte-for-byte unchanged.
pub struct Reporter {
    header: Option<ProjectHeader>,
    interactive: bool,
}

impl Reporter {
    pub fn new(header: Option<ProjectHeader>) -> Self {
        let interactive = header.is_some() && stderr_is_colour_capable();
        Reporter {
            header,
            interactive,
        }
    }

    /// `Compiling {name} {version} ({root})`, before the entry file is read.
    pub fn compiling(&self) {
        if let Some(h) = &self.header {
            eprintln!("Compiling {} {} ({})", h.name, h.version, h.root);
        }
    }

    /// The one `Building` line: printed after the static phases succeeded,
    /// immediately before the warnings they produced.
    pub fn building_done(&self) {
        if self.header.is_none() {
            return;
        }
        if self.interactive {
            let bar = "\u{2588}".repeat(WIDTH);
            eprintln!("Building [{}] 100% ({}/{})", bar, STEPS, STEPS);
        } else {
            let bar = "#".repeat(WIDTH);
            eprintln!("Building [{}] 100% ({}/{})", bar, STEPS, STEPS);
        }
    }

    /// `Running`, then the one blank line the program's output follows.
    pub fn running(&self) {
        if self.header.is_some() {
            eprintln!("Running");
            eprintln!();
        }
    }
}

/// Colour on **stderr**, the stream this module writes: off when `NO_COLOR` is
/// set, when `TERM=dumb`, and whenever stderr is not a terminal.
/// `std::io::IsTerminal` is stable since Rust 1.70 and the lock file is format
/// 4 (`Cargo.lock:3`), so the toolchain is new enough.
fn stderr_is_colour_capable() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if std::env::var("TERM").map(|t| t == "dumb").unwrap_or(false) {
        return false;
    }
    std::io::stderr().is_terminal()
}
