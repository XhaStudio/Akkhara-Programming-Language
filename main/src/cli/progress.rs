//! The `akk run` project header: `Compiling`, `Building`, `Running`.
//!
//! The words describe work this interpreter really does, not a code-generation
//! step: "Compiling" is the entry file being read, lexed and parsed;
//! "Building" is the static phase (`library_warnings`, which also reads and
//! parses every script a program brings in, plus the `use`/dependency
//! resolution `cmd_run` already performs); "Running" is the compiled file
//! executing.
//!
//! The bar is a **phase indicator printed once**, after the static phase has
//! finished -- not a live measure of work in progress: there is no redraw, no
//! timer, no cursor hiding.
//!
//! Styling is part of the same switch as the bar: on a terminal (and with
//! `NO_COLOR` unset, `TERM` not `dumb`) the words are bold and the numbers
//! are coloured; a piped stream gets the same lines as plain text.

use std::io::IsTerminal;

/// The phase words, in one place so a rename is a one-line change.
pub const COMPILING: &str = "Compiling";
pub const BUILDING: &str = "Building";
pub const RUNNING: &str = "Running";
pub const FINISHED: &str = "Finished";

// SGR parameters. Bold is its own; the coloured words are bold *and* coloured.
const BOLD: &str = "1";
const BLUE: &str = "1;34";
const CYAN: &str = "1;36";
const GREEN: &str = "1;32";
const RESET: &str = "\u{1b}[0m";

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
    blocks: bool,
    color: bool,
}

impl Reporter {
    pub fn new(header: Option<ProjectHeader>) -> Self {
        let control = stderr_allows_control();
        let blocks = use_blocks(
            header.is_some() && control,
            cfg!(windows),
            std::env::var_os("WT_SESSION").is_some(),
        );
        Reporter {
            header,
            blocks,
            color: control,
        }
    }

    /// `Compiling {name} {version} ({root})`, before the entry file is read:
    /// the word and the folder are bold, the version bold blue.
    pub fn compiling(&self) {
        if let Some(h) = &self.header {
            eprintln!(
                "{} {} {} ({})",
                paint(self.color, BOLD, COMPILING),
                paint(self.color, BOLD, &h.name),
                paint(self.color, BLUE, &h.version),
                h.root
            );
        }
    }

    /// The one `Building` line: printed after the static phase finished,
    /// immediately before the warnings it produced are written. Every number
    /// on it is cyan.
    pub fn building_done(&self) {
        if self.header.is_none() {
            return;
        }
        let cell = if self.blocks { "\u{2588}" } else { "#" };
        let bar = cell.repeat(WIDTH);
        eprintln!(
            "{} [{}] {}% ({}/{})",
            BUILDING,
            bar,
            number(self.color, "100"),
            number(self.color, &STEPS.to_string()),
            number(self.color, &STEPS.to_string())
        );
    }

    /// `Running`, then the one blank line the program's output follows.
    pub fn running(&self) {
        if self.header.is_some() {
            eprintln!("{}", RUNNING);
            eprintln!();
        }
    }

    /// `Finished {path} ({bytes} bytes)`: `akk build`'s closing line, printed
    /// once the file is complete and verified. Not tied to a project header:
    /// a build has none and still reports what it wrote. The word is bold
    /// green and the byte count cyan.
    pub fn finished(&self, path: &str, bytes: u64) {
        eprintln!(
            "{} {} ({} bytes)",
            paint(self.color, GREEN, FINISHED),
            path,
            number(self.color, &bytes.to_string())
        );
    }
}

/// `Compiled in {seconds}s` on stdout: the one timing line, shown by a build
/// and by a run that had to compile. The number is cyan on a terminal.
pub fn compiled_in(seconds: f64) {
    let text = format!("{:.3}s", seconds);
    println!("Compiled in {}", number(stdout_allows_control(), &text));
}

/// Wrap `text` in an SGR colour when `enabled`, otherwise return it as-is.
fn paint(enabled: bool, sgr: &str, text: &str) -> String {
    if enabled {
        format!("\u{1b}[{}m{}{}", sgr, text, RESET)
    } else {
        text.to_string()
    }
}

/// A count on a styled line: bold cyan.
fn number(enabled: bool, text: &str) -> String {
    paint(enabled, CYAN, text)
}

/// Block characters only on a stream we are drawing to that will render them:
/// a terminal, and not a Windows console without Windows Terminal (`WT_SESSION`
/// unset means the legacy conhost, whose default code page does not).
///
/// Kept pure so every combination is unit-tested.
fn use_blocks(interactive: bool, windows: bool, wt_session: bool) -> bool {
    interactive && !(windows && !wt_session)
}

/// Whether a stream may carry colour and bold at all: not a pipe, `TERM=dumb`,
/// or a non-empty `NO_COLOR` (no-color.org: an empty value counts as unset)
/// all turn it off. `std::io::IsTerminal` is stable since Rust 1.70, and the
/// lock file is format 4 (`Cargo.lock:3`), so the toolchain is new enough.
fn stream_allows_control(terminal: bool) -> bool {
    if !terminal {
        return false;
    }
    let no_color = std::env::var_os("NO_COLOR")
        .map(|value| !value.is_empty())
        .unwrap_or(false);
    if no_color {
        return false;
    }
    !std::env::var("TERM").map(|t| t == "dumb").unwrap_or(false)
}

fn stderr_allows_control() -> bool {
    stream_allows_control(std::io::stderr().is_terminal())
}

fn stdout_allows_control() -> bool {
    stream_allows_control(std::io::stdout().is_terminal())
}

#[cfg(test)]
mod tests {
    use super::{number, paint, stream_allows_control, use_blocks, BOLD, CYAN};

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
    fn windows_console_and_non_windows_use_blocks() {
        assert!(use_blocks(true, true, true));
        assert!(use_blocks(true, false, false));
    }

    #[test]
    fn styling_is_only_added_when_enabled() {
        assert_eq!(paint(false, BOLD, "Compiling"), "Compiling");
        assert_eq!(number(false, "100"), "100");
        assert_eq!(
            paint(true, BOLD, "Compiling"),
            "\u{1b}[1mCompiling\u{1b}[0m"
        );
        assert_eq!(number(true, "100"), "\u{1b}[1;36m100\u{1b}[0m");
        assert_eq!(
            number(true, "1"),
            format!("\u{1b}[{}m1\u{1b}[0m", CYAN),
            "numbers share one colour"
        );
    }

    #[test]
    fn a_pipe_never_gets_control_sequences() {
        assert!(!stream_allows_control(false));
    }
}
