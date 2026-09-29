//! End-to-end tests for the `akk` binary.
//!
//! Each case runs the compiled binary against a program in `tests/fixtures/`
//! and checks three things: the exit code, the exact stdout, and (for failing
//! programs) the error code reported on stderr. The fixtures deliberately
//! avoid randomness, wall-clock waits, and the network so the suite is fast
//! and deterministic: random calls are pinned to a single-element list or an
//! end-exclusive `(min, max)` pair with `max == min + 1`, and the `request`
//! fixtures stop at the "not imported" guard, before any I/O happens.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// One fixture and the behavior running it should produce.
struct Case {
    /// Fixture file stem, e.g. `"hello"` for `tests/fixtures/hello.akk`.
    name: &'static str,
    /// Expected process exit code (0 = success, 1 = error).
    exit: i32,
    /// Expected stdout, line by line (no trailing newline needed).
    stdout: &'static [&'static str],
    /// An error code that must appear in stderr, if the program should fail.
    stderr_code: Option<&'static str>,
    /// Lines piped to the program's stdin, one per line (default: empty).
    stdin: &'static [&'static str],
}

const CASES: &[Case] = &[
    // --- programs that run to completion -----------------------------------
    Case {
        name: "hello",
        exit: 0,
        stdout: &["Hello, World!"],
        stderr_code: None,
        stdin: &[],
    },
    Case {
        name: "control_flow",
        exit: 0,
        stdout: &["two", "0", "2", "4", "7", "8", "1", "2", "3"],
        stderr_code: None,
        stdin: &[],
    },
    Case {
        name: "functions",
        exit: 0,
        stdout: &["Hello, World!", "42"],
        stderr_code: None,
        stdin: &[],
    },
    Case {
        name: "collections",
        exit: 0,
        stdout: &["20", "2", "{1, 2, 3}", "7", "f=", "7.5"],
        stderr_code: None,
        stdin: &[],
    },
    Case {
        name: "lib_call_forms",
        exit: 0,
        stdout: &["A", "A", "A", "5", "5"],
        stderr_code: None,
        stdin: &[],
    },
    Case {
        name: "lib_alias",
        exit: 0,
        stdout: &["Syntax1:", "A", "Syntax2:", "B", "Form1:", "C"],
        stderr_code: None,
        stdin: &[],
    },
    // eng library: English-spelled typed declarations alongside Myanmar ones
    Case {
        name: "eng_declare",
        exit: 0,
        stdout: &["John", "25", "5.9", "True", "100", "adult", "မင်္ဂလာပါ", "30"],
        stderr_code: None,
        stdin: &[],
    },
    Case {
        name: "eng_catch",
        exit: 0,
        stdout: &["E104 reassign ok", "E103 type ok", "E105 redeclare ok", "10"],
        stderr_code: None,
        stdin: &[],
    },
    // eng library: English-spelled if / while blocks and fn definitions
    Case {
        name: "eng_control",
        exit: 0,
        stdout: &[
            "adult",
            "0",
            "1",
            "2",
            "and ok",
            "or ok",
            "mm saw three",
            "eng saw three",
            "nested ok",
        ],
        stderr_code: None,
        stdin: &[],
    },
    Case {
        name: "eng_fn",
        exit: 0,
        stdout: &[
            "Hello, David!",
            "5",
            "negative",
            "zero",
            "55",
            "42",
            "John",
            "Hello, မြန်မာ!",
        ],
        stderr_code: None,
        stdin: &[],
    },
    // eng library: loop { ... } with break / break <value> / ရပ်ပါ
    Case {
        name: "eng_loop",
        exit: 0,
        stdout: &["12", "4", "5", "2", "6", "3"],
        stderr_code: None,
        stdin: &[],
    },
    Case {
        name: "eng_loop_catch",
        exit: 0,
        stdout: &["E116 ok", "E115 ok", "E103 ok", "1"],
        stderr_code: None,
        stdin: &[],
    },
    // eng library: `use <lib>;` imports and `<lib>.<fn>(args);` calls
    Case {
        name: "eng_lib",
        exit: 0,
        stdout: &["5", "7", "A", "1", "True"],
        stderr_code: None,
        stdin: &[],
    },
    Case {
        name: "eng_lib_catch",
        exit: 0,
        stdout: &["E089 ok", "E085 ok", "E117 ok", "done"],
        stderr_code: None,
        stdin: &[],
    },
    // eng library: the core builtin helpers (len / abs / min / ... / contains)
    Case {
        name: "eng_builtin",
        exit: 0,
        stdout: &[
            "3", "5", "7", "2.5", "2", "9", "1", "3", "4.0", "2.5", "3", "4", "4", "ENG",
            "english", "Akkhara", "True", "True", "4", "sqrt ok",
        ],
        stderr_code: None,
        stdin: &[],
    },
    // --- programs that catch their own errors ------------------------------
    Case {
        name: "error_catch",
        exit: 0,
        stdout: &[
            "E091 ok", "E092 ok", "E085 ok", "E086 ok", "E087 ok", "E064 ok", "E031 ok",
        ],
        stderr_code: None,
        stdin: &[],
    },
    Case {
        name: "request_not_imported",
        exit: 0,
        stdout: &["E078 get ok", "E078 post ok", "E089 ok"],
        stderr_code: None,
        stdin: &[],
    },
    // --- programs that fail ------------------------------------------------
    Case {
        name: "parse_alias_missing",
        exit: 1,
        stdout: &[],
        stderr_code: Some("E093"),
        stdin: &[],
    },
    Case {
        name: "parse_missing_period",
        exit: 1,
        stdout: &[],
        stderr_code: Some("E002"),
        stdin: &[],
    },
    Case {
        name: "parse_syntax_error",
        exit: 1,
        stdout: &[],
        stderr_code: Some("E001"),
        stdin: &[],
    },
    Case {
        name: "eng_parse_error",
        exit: 1,
        stdout: &[],
        stderr_code: Some("E100"),
        stdin: &[],
    },
    Case {
        name: "eng_type_error",
        exit: 1,
        stdout: &[],
        stderr_code: Some("E101"),
        stdin: &[],
    },
    Case {
        name: "eng_return_error",
        exit: 1,
        stdout: &["before"],
        stderr_code: Some("E108"),
        stdin: &[],
    },
    Case {
        name: "eng_break_error",
        exit: 1,
        stdout: &["before"],
        stderr_code: Some("E116"),
        stdin: &[],
    },
    Case {
        name: "eng_use_error",
        exit: 1,
        stdout: &[],
        stderr_code: Some("E118"),
        stdin: &[],
    },
    Case {
        name: "runtime_uncaught",
        exit: 1,
        stdout: &["before"],
        stderr_code: Some("E031"),
        stdin: &[],
    },
    // eng library: print()/input()
    Case {
        name: "eng_print",
        exit: 0,
        stdout: &[
            "Hello, world!",
            "David",
            "30",
            "1.75",
            "True",
            "42",
            "Hi, David!",
            "David",
        ],
        stderr_code: None,
        stdin: &[],
    },
    Case {
        name: "eng_input",
        exit: 0,
        // Prompts are printed without a trailing newline, so each prompt
        // shares its line with the (echoed by the terminal) typed input.
        stdout: &[
            ">> What is your name?: David",
            "Give a number: 50",
            "Ratio: 3.5",
            "Ready? (true/false): False",
        ],
        stderr_code: None,
        // The first line is consumed by the prompt-only `input(">> ")`.
        stdin: &["skip me", "David", "42", "3.5", "false"],
    },
];

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

/// Run the binary with the given arguments.
fn run<S: AsRef<OsStr>>(args: &[S]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_akk"));
    for arg in args {
        cmd.arg(arg);
    }
    cmd.output().expect("failed to spawn the akk binary")
}

/// Run the binary on a fixture file. `stdin_lines`, when non-empty, is
/// piped to the program's stdin one line at a time (the pipe is closed
/// afterward, so reads hit EOF); otherwise stdin is /dev/null.
fn run_fixture(name: &str, stdin_lines: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_akk"));
    cmd.arg(fixtures_dir().join(format!("{name}.akk")));
    if stdin_lines.is_empty() {
        cmd.stdin(std::process::Stdio::null());
        return cmd.output().expect("failed to spawn the akk binary");
    }
    cmd.stdin(std::process::Stdio::piped());
    // Unlike output(), spawn() leaves stdout/stderr inherited by default,
    // so pipe them explicitly or wait_with_output() would collect nothing.
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().expect("failed to spawn the akk binary");
    use std::io::Write;
    let mut stdin = child.stdin.take().expect("stdin");
    for line in stdin_lines {
        writeln!(stdin, "{line}").expect("failed to write to stdin");
    }
    stdin.flush().expect("failed to flush stdin");
    drop(stdin); // close the pipe so the child's reads see EOF
    let output = child.wait_with_output().expect("failed to wait for the akk binary");
    output
}

fn text(bytes: &[u8]) -> String {
    // Normalize CRLF so the expectations hold on every platform.
    String::from_utf8_lossy(bytes).replace("\r\n", "\n")
}

fn expected_stdout(lines: &[&str]) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

#[test]
fn fixtures_match_expected_behavior() {
    let mut failures = Vec::new();

    for case in CASES {
        let output = run_fixture(case.name, case.stdin);
        let exit = output.status.code().unwrap_or(-1);
        let stdout = text(&output.stdout);
        let stderr = text(&output.stderr);

        if exit != case.exit {
            failures.push(format!(
                "{}: exit code {exit}, expected {}\n    stderr: {}",
                case.name,
                case.exit,
                stderr.trim()
            ));
        }

        let want_stdout = expected_stdout(case.stdout);
        if stdout != want_stdout {
            failures.push(format!(
                "{}: stdout was {:?}, expected {:?}",
                case.name, stdout, want_stdout
            ));
        }

        match case.stderr_code {
            Some(code) if !stderr.contains(code) => failures.push(format!(
                "{}: stderr did not contain {code}\n    stderr: {}",
                case.name,
                stderr.trim()
            )),
            None if !stderr.is_empty() => failures.push(format!(
                "{}: unexpected stderr: {}",
                case.name,
                stderr.trim()
            )),
            _ => {}
        }
    }

    assert!(
        failures.is_empty(),
        "{} fixture(s) failed:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// Every `.akk` file under `tests/fixtures/` should be registered above, so a
/// new fixture can't be added without also saying what it should do.
#[test]
fn every_fixture_is_covered() {
    let mut on_disk: Vec<String> = std::fs::read_dir(fixtures_dir())
        .expect("tests/fixtures/ should exist")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension()? != "akk" {
                return None;
            }
            Some(path.file_stem()?.to_string_lossy().into_owned())
        })
        .collect();
    on_disk.sort();

    let mut registered: Vec<String> = CASES.iter().map(|c| c.name.to_string()).collect();
    registered.sort();

    assert_eq!(
        on_disk, registered,
        "add every fixture in tests/fixtures/ to the CASES list in tests/cli.rs"
    );
}

#[test]
fn version_flag_reports_the_cargo_version() {
    let output = run(&["--version"]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = text(&output.stdout);
    assert_eq!(stdout.trim(), format!("akk {}", env!("CARGO_PKG_VERSION")));
}

#[test]
fn help_exits_successfully() {
    let output = run(&["--help"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(text(&output.stderr).contains("Usage: akk"));
}

#[test]
fn no_arguments_prints_usage_and_fails() {
    let output = run(&[] as &[&str]);
    assert_eq!(output.status.code(), Some(1));
    assert!(text(&output.stderr).contains("Usage: akk"));
}

#[test]
fn missing_file_fails_with_a_message() {
    let output = run(&[fixtures_dir().join("does_not_exist.akk")]);
    assert_eq!(output.status.code(), Some(1));
    assert!(!text(&output.stderr).trim().is_empty());
}
