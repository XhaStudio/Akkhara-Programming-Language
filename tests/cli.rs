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
}

const CASES: &[Case] = &[
    // --- programs that run to completion -----------------------------------
    Case {
        name: "hello",
        exit: 0,
        stdout: &["Hello, World!"],
        stderr_code: None,
    },
    Case {
        name: "control_flow",
        exit: 0,
        stdout: &["two", "0", "2", "4", "7", "8", "1", "2", "3"],
        stderr_code: None,
    },
    Case {
        name: "functions",
        exit: 0,
        stdout: &["Hello, World!", "42"],
        stderr_code: None,
    },
    Case {
        name: "collections",
        exit: 0,
        stdout: &["20", "2", "{1, 2, 3}", "7", "f=", "7.5"],
        stderr_code: None,
    },
    Case {
        name: "lib_call_forms",
        exit: 0,
        stdout: &["A", "A", "A", "5", "5"],
        stderr_code: None,
    },
    Case {
        name: "lib_alias",
        exit: 0,
        stdout: &["Syntax1:", "A", "Syntax2:", "B", "Form1:", "C"],
        stderr_code: None,
    },
    // --- programs that catch their own errors ------------------------------
    Case {
        name: "error_catch",
        exit: 0,
        stdout: &[
            "E091 ok", "E092 ok", "E085 ok", "E086 ok", "E087 ok", "E064 ok", "E031 ok",
        ],
        stderr_code: None,
    },
    Case {
        name: "request_not_imported",
        exit: 0,
        stdout: &["E078 get ok", "E078 post ok", "E089 ok"],
        stderr_code: None,
    },
    // --- programs that fail ------------------------------------------------
    Case {
        name: "parse_alias_missing",
        exit: 1,
        stdout: &[],
        stderr_code: Some("E093"),
    },
    Case {
        name: "parse_missing_period",
        exit: 1,
        stdout: &[],
        stderr_code: Some("E002"),
    },
    Case {
        name: "parse_syntax_error",
        exit: 1,
        stdout: &[],
        stderr_code: Some("E001"),
    },
    Case {
        name: "runtime_uncaught",
        exit: 1,
        stdout: &["before"],
        stderr_code: Some("E031"),
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

/// Run the binary on a fixture file.
fn run_fixture(name: &str) -> Output {
    run(&[fixtures_dir().join(format!("{name}.akk"))])
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
        let output = run_fixture(case.name);
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
