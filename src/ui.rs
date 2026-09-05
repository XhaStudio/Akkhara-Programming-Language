//! Small terminal UI helpers used by `akk install` / `akk update`:
//!   - `confirm`     an arrow-key-driven "Yes"/"No" prompt
//!   - `with_spinner` an animated spinner shown while a task runs
//!
//! Both degrade gracefully (plain text, no ANSI/raw-mode) when stdout/stdin
//! isn't a real terminal -- e.g. when akk is run from a script or CI.

use std::io::{stdin, stdout, BufRead, Stdout, Write};
use std::process;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crossterm::cursor::{Hide, MoveToColumn, Show};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor};
use crossterm::terminal::{self, Clear, ClearType};
use crossterm::{execute, queue};

/// Braille spinner frames -- a smooth, compact "loading" animation.
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Shows `question` with a "Yes" / "No" choice the user moves between with
/// the Left/Right/Up/Down arrow keys and accepts with Enter (y/n and Esc
/// also work as shortcuts). Returns `true` for "Yes".
///
/// Falls back to a plain `[Y/n]` text prompt if raw mode can't be enabled
/// (piped input, no real terminal, etc.) so akk still works non-interactively.
pub fn confirm(question: &str) -> bool {
    if terminal::enable_raw_mode().is_err() {
        return confirm_fallback(question);
    }

    let mut out = stdout();
    let _ = execute!(out, Hide);

    let mut selected_yes = true;
    let result = loop {
        draw_confirm(&mut out, question, selected_yes);

        match event::read() {
            Ok(Event::Key(key)) if key.kind != KeyEventKind::Release => match key.code {
                KeyCode::Left | KeyCode::Up => selected_yes = true,
                KeyCode::Right | KeyCode::Down => selected_yes = false,
                KeyCode::Tab => selected_yes = !selected_yes,
                KeyCode::Enter => break selected_yes,
                KeyCode::Char('y') | KeyCode::Char('Y') => break true,
                KeyCode::Char('n') | KeyCode::Char('N') => break false,
                KeyCode::Esc => break false,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break false,
                _ => {}
            },
            Ok(_) => {}
            Err(_) => break selected_yes,
        }
    };

    let _ = queue!(out, Print("\r\n"), Show);
    let _ = out.flush();
    let _ = terminal::disable_raw_mode();
    result
}

fn draw_confirm(out: &mut Stdout, question: &str, selected_yes: bool) {
    let _ = queue!(out, Clear(ClearType::CurrentLine), MoveToColumn(0));
    let _ = queue!(
        out,
        SetForegroundColor(Color::Cyan),
        Print("?  "),
        ResetColor,
        SetAttribute(Attribute::Bold),
        Print(question),
        SetAttribute(Attribute::Reset),
        Print("  "),
    );

    if selected_yes {
        let _ = queue!(
            out,
            SetAttribute(Attribute::Reverse),
            SetForegroundColor(Color::Green),
            Print(" Yes "),
            ResetColor,
            SetAttribute(Attribute::Reset),
            Print("   "),
            SetForegroundColor(Color::DarkGrey),
            Print(" No "),
            ResetColor,
        );
    } else {
        let _ = queue!(
            out,
            SetForegroundColor(Color::DarkGrey),
            Print(" Yes "),
            ResetColor,
            Print("   "),
            SetAttribute(Attribute::Reverse),
            SetForegroundColor(Color::Red),
            Print(" No "),
            ResetColor,
            SetAttribute(Attribute::Reset),
        );
    }

    let _ = queue!(
        out,
        SetForegroundColor(Color::DarkGrey),
        Print("   (\u{2190}/\u{2192}/\u{2191}/\u{2193} choose \u{00b7} Enter confirm)"),
        ResetColor,
    );
    let _ = out.flush();
}

fn confirm_fallback(question: &str) -> bool {
    print!("?  {}  [Y/n] ", question);
    let _ = stdout().flush();

    let mut line = String::new();
    if stdin().lock().read_line(&mut line).is_err() {
        return false;
    }

    let answer = line.trim().to_lowercase();
    answer.is_empty() || answer == "y" || answer == "yes"
}

/// Runs `task` on a background thread while animating a spinner next to
/// `message` in the terminal, then returns whatever `task` returns.
///
/// Falls back to printing `message` once (no animation) when the terminal
/// doesn't support cursor control, e.g. piped output or CI.
pub fn with_spinner<T, F>(message: &str, task: F) -> T
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (done_tx, done_rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        let result = task();
        let _ = done_tx.send(());
        result
    });

    let mut out = stdout();
    let interactive = execute!(out, Hide).is_ok();

    if !interactive {
        println!("{}", message);
    }

    let mut frame = 0usize;
    loop {
        if interactive {
            let _ = queue!(out, Clear(ClearType::CurrentLine), MoveToColumn(0));
            let _ = queue!(
                out,
                SetForegroundColor(Color::Cyan),
                Print(SPINNER_FRAMES[frame % SPINNER_FRAMES.len()]),
                ResetColor,
                Print(" "),
                Print(message),
            );
            let _ = out.flush();
            frame += 1;
        }

        match done_rx.recv_timeout(Duration::from_millis(80)) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
        }
    }

    if interactive {
        let _ = queue!(
            out,
            Clear(ClearType::CurrentLine),
            MoveToColumn(0),
            SetForegroundColor(Color::Green),
            Print("\u{2713} "),
            ResetColor,
            Print(message),
            Print("\n"),
            Show,
        );
        let _ = out.flush();
    }

    handle.join().unwrap_or_else(|_| {
        eprintln!("    [FAILED] background task panicked");
        process::exit(1);
    })
}
