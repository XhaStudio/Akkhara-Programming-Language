//! The `akk run` project header: `Compiling`, `Building`, `Running`.
//!
//! The words describe work this interpreter really does, not a code-generation
//! step: "Compiling" is the entry file being read, lexed and parsed;
//! "Building" is the static phase (`library_warnings`, which also reads and
//! parses every script a program brings in, plus the `use`/dependency
//! resolution `cmd_run` already performs); "Running" is the interpreter
//! executing the parsed program.
//!
//! The bar is a **phase indicator printed once**, after the static phase has
//! finished -- not a live measure of work in progress: there is no redraw, no
//! timer, no cursor hiding. A future `akk build` can reuse `Reporter`
//! unchanged.

use std::io::IsTerminal;

/// What the header prints: `[package] name`, `[package] version`, and the
/// project root, already rendered as a display string.
pub struct ProjectHeader {
    pub name: String,
    pub version: String,
    pub root: String,
}

/// Phases the status covers: entry read, lex+parse, analysis+resolution.
const STEPS: usize = 3;
/// Width of the rendered bar, identical in both renderings.
const WIDTH: usize = 10;

/// Writes the phase lines to stderr. With `header == None` every method is a
/// no-op, so `akk file.akk`, `akk run <file.akk>` and `akk report` are
/// byte-for-byte unchanged.
pub struct Reporter {
    header: Option<ProjectHeader>,
    interactive: bool,
    blocks: bool,
}

impl Reporter {
    pub fn new(header: Option<ProjectHeader>) -> Self {
        let interactive = header.is_some() && stderr_allows_control();
        let blocks = use_blocks(
            interactive,
            cfg!(windows),
            std::env::var_os("WT_SESSION").is_some(),
        );
        Reporter {
            header,
            interactive,
            blocks,
        }
    }

    /// `Compiling {name} {version} ({root})`, before the entry file is read.
    pub fn compiling(&self) {
        if let Some(h) = &self.header {
            eprintln!("Compiling {} {} ({})", h.name, h.version, h.root);
        }
    }

    /// The one `Building` line: printed after the static phase finished,
    /// immediately before the warnings it produced are written.
    pub fn building_done(&self) {
        if self.header.is_none() {
            return;
        }
        let cell = if self.blocks { "\u{2588}" } else { "#" };
        let bar = cell.repeat(WIDTH);
        eprintln!("Building [{}] 100% ({}/{})", bar, STEPS, STEPS);
    }

    /// `Running`, then the one blank line the program's output follows.
    pub fn running(&self) {
        if self.header.is_some() {
            eprintln!("Running");
            eprintln!();
        }
    }
}

/// Block characters only on a stream we are drawing to that will render them:
/// a terminal, and not a Windows console without Windows Terminal (`WT_SESSION`
/// unset means the legacy conhost, whose default code page does not).
///
/// Kept pure so every combination is unit-tested.
fn use_blocks(interactive: bool, windows: bool, wt_session: bool) -> bool {
    interactive && !(windows && !wt_session)
}

/// Whether stderr may carry styled output at all: not a terminal, `TERM=dumb`,
/// or a non-empty `NO_COLOR` (no-color.org: an empty value counts as unset) all
/// turn it off. `std::io::IsTerminal` is stable since Rust 1.70, and the lock
/// file is format 4 (`Cargo.lock:3`), so the toolchain is new enough.
fn stderr_allows_control() -> bool {
    let no_color = std::env::var_os("NO_COLOR")
        .map(|value| !value.is_empty())
        .unwrap_or(false);
    if no_color {
        return false;
    }
    if std::env::var("TERM").map(|t| t == "dumb").unwrap_or(false) {
        return false;
    }
    std::io::stderr().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::use_blocks;

    #[test]
    fn blocks_need_an_interactive_stream() {
        assert!(!use_blocks(false, false, false));
        assert!(!use_blocks(false, true, true));
    }

    #[test]
    fn windows_console_without_windows_terminal_uses_ascii() {
        assert!(!use_blocks(true, true, false));
    }

    #[test]
    fn windows_terminal_and_non_windows_use_blocks() {
        assert!(use_blocks(true, true, true));
        assert!(use_blocks(true, false, false));
    }
}
