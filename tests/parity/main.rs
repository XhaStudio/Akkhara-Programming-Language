//! Native parity harness.
//!
//! Every fixture under `tests/fixtures/` is built by the native backend and
//! compared with the interpreter. `tests/parity/expectations.txt` records, per
//! fixture, one of two outcomes:
//!
//! * `match`: the native build must succeed and produce the interpreter's
//!   exit status, stdout and stderr.
//! * `refuse`: the native build must fail with an `Exxx` diagnostic naming
//!   the construct it cannot compile yet. A compiler panic is not a refusal.
//!
//! A fixture the native build silently accepts but gets wrong therefore fails
//! here instead of shipping. Run just this harness with:
//!
//! ```text
//! cargo test --test parity
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const AKK: &str = env!("CARGO_BIN_EXE_akk");
const RUN_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
enum Expect {
    /// Both engines must agree.
    Match,
    /// The native build must fail with a diagnostic containing these words.
    Refuse(Vec<String>),
}

#[derive(Debug)]
struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Run {
    fn from_output(output: Output) -> Self {
        Self {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixtures_dir() -> PathBuf {
    manifest_dir().join("tests").join("fixtures")
}

fn expectations_path() -> PathBuf {
    manifest_dir()
        .join("tests")
        .join("parity")
        .join("expectations.txt")
}

/// Read `expectations.txt`. Blank lines and `#` comments are ignored; fields
/// are tab-separated: `name`, then `match` or `refuse` plus its keywords.
fn expectations() -> BTreeMap<String, Expect> {
    let path = expectations_path();
    let text =
        fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    let mut out = BTreeMap::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').map(str::trim).collect();
        let name = fields.first().copied().unwrap_or_default();
        let kind = fields.get(1).copied().unwrap_or_default();
        assert!(
            !name.is_empty(),
            "line {}: missing fixture name",
            number + 1
        );
        let expectation = match kind {
            "match" => {
                assert_eq!(
                    fields.len(),
                    2,
                    "line {}: `match` takes no keywords",
                    number + 1
                );
                Expect::Match
            }
            "refuse" => {
                let keywords: Vec<String> = fields[2..]
                    .iter()
                    .filter(|field| !field.is_empty())
                    .map(|field| field.to_string())
                    .collect();
                assert!(
                    !keywords.is_empty(),
                    "line {}: a refusal needs at least one keyword",
                    number + 1
                );
                Expect::Refuse(keywords)
            }
            other => panic!(
                "line {}: expected `match` or `refuse`, found {other:?}",
                number + 1
            ),
        };
        assert!(
            out.insert(name.to_string(), expectation).is_none(),
            "line {}: {name} is listed twice",
            number + 1
        );
    }
    out
}

/// Run a command with no stdin and a bounded wait, so a fixture that waits for
/// input or never exits fails the harness instead of hanging the suite.
fn run(command: &mut Command) -> Result<Output, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|err| format!("could not start {:?}: {err}", command))?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child
                    .wait_with_output()
                    .map_err(|err| format!("could not read output: {err}"))
            }
            Ok(None) if start.elapsed() > RUN_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("did not exit within {RUN_TIMEOUT:?}"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(err) => return Err(format!("could not wait for the process: {err}")),
        }
    }
}

/// A `Wxxx` line is a compile warning; the native backend does not repeat
/// warnings, so they are not part of behavioral parity.
fn is_warning(line: &str) -> bool {
    let bytes = line.as_bytes();
    bytes.len() > 4
        && bytes[0] == b'W'
        && bytes[1..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b' '
}

/// The interpreter reports "Compiled in ..." with a wall-clock duration on
/// some paths; that line says nothing about the program's behavior.
fn cleaned(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.starts_with("Compiled in ") && !is_warning(line))
        .collect();
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

/// Compare the two engines. Anything that differs -- including an exit status
/// or a single character of output -- is a failure.
fn compare_runs(interpreted: &Run, native: &Run) -> Result<(), String> {
    if interpreted.code != native.code {
        return Err(format!(
            "exit status differs: interpreter {:?}, native {:?}",
            interpreted.code, native.code
        ));
    }
    if cleaned(&interpreted.stdout) != cleaned(&native.stdout) {
        return Err(format!(
            "stdout differs:\n  interpreter: {:?}\n  native:      {:?}",
            cleaned(&interpreted.stdout),
            cleaned(&native.stdout)
        ));
    }
    if cleaned(&interpreted.stderr) != cleaned(&native.stderr) {
        return Err(format!(
            "stderr differs:\n  interpreter: {:?}\n  native:      {:?}",
            cleaned(&interpreted.stderr),
            cleaned(&native.stderr)
        ));
    }
    Ok(())
}

/// The first `Exxx` token in a diagnostic, if there is one.
fn diagnostic_code(stderr: &str) -> Option<&str> {
    stderr.split_whitespace().find(|token| {
        token.len() == 4 && token.starts_with('E') && token[1..].chars().all(|c| c.is_ascii_digit())
    })
}

/// Check that a failed build refused honestly: a numbered diagnostic naming
/// the expected construct, not a panic and not silence.
fn check_refusal(stderr: &str, keywords: &[String]) -> Result<(), String> {
    if stderr.contains("panicked at") || stderr.contains("RUST_BACKTRACE") {
        return Err(format!(
            "the compiler panicked instead of refusing:\n{stderr}"
        ));
    }
    if stderr.trim().is_empty() {
        return Err("the build failed without a diagnostic".to_string());
    }
    if diagnostic_code(stderr).is_none() {
        return Err(format!("the diagnostic has no Exxx code:\n{stderr}"));
    }
    for keyword in keywords {
        if !stderr.contains(keyword) {
            return Err(format!(
                "the diagnostic does not name {keyword:?}:\n{stderr}"
            ));
        }
    }
    Ok(())
}

fn executable_for(dir: &PathBuf, name: &str) -> PathBuf {
    dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

#[test]
fn every_fixture_matches_the_interpreter_or_is_refused() {
    if !cfg!(target_arch = "x86_64") {
        eprintln!("skipping: the native backend writes x86_64 only");
        return;
    }

    let expectations = expectations();
    let mut names: Vec<String> = fs::read_dir(fixtures_dir())
        .expect("tests/fixtures/ should exist")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension()? != "akk" {
                return None;
            }
            Some(path.file_stem()?.to_string_lossy().into_owned())
        })
        .collect();
    names.sort();

    let on_disk: BTreeSet<String> = names.iter().cloned().collect();
    let listed: BTreeSet<String> = expectations.keys().cloned().collect();
    assert_eq!(
        on_disk, listed,
        "tests/parity/expectations.txt must list every fixture exactly once"
    );

    let root = std::env::temp_dir().join(format!("akk-parity-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create the parity work directory");

    let mut failures = Vec::new();
    for name in &names {
        let dir = root.join(name);
        fs::create_dir_all(&dir).expect("create a fixture directory");
        let file = format!("{name}.akk");
        fs::copy(fixtures_dir().join(&file), dir.join(&file))
            .unwrap_or_else(|err| panic!("copy {file}: {err}"));

        let build = run(Command::new(AKK)
            .arg("build")
            .arg(&file)
            .current_dir(&dir)
            .env("NO_COLOR", "1"));

        match &expectations[name] {
            Expect::Refuse(keywords) => {
                let output = match build {
                    Ok(output) => output,
                    Err(err) => {
                        failures.push(format!("{name}: native build: {err}"));
                        continue;
                    }
                };
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    failures.push(format!(
                        "{name}: marked refuse, but the native build succeeded: {}",
                        stdout.trim()
                    ));
                    continue;
                }
                if executable_for(&dir, name).exists() {
                    failures.push(format!(
                        "{name}: refused, but an executable was still written"
                    ));
                }
                let stderr = String::from_utf8_lossy(&output.stderr);
                if let Err(err) = check_refusal(&stderr, keywords) {
                    failures.push(format!("{name}: {err}"));
                }
            }
            Expect::Match => {
                let output = match build {
                    Ok(output) => output,
                    Err(err) => {
                        failures.push(format!("{name}: native build: {err}"));
                        continue;
                    }
                };
                if !output.status.success() {
                    failures.push(format!(
                        "{name}: marked match, but the native build failed:\n{}",
                        String::from_utf8_lossy(&output.stderr)
                    ));
                    continue;
                }
                let executable = executable_for(&dir, name);
                if !executable.exists() {
                    failures.push(format!("{name}: the build wrote no executable"));
                    continue;
                }

                let interpreted = run(Command::new(AKK)
                    .arg(&file)
                    .current_dir(&dir)
                    .env("NO_COLOR", "1"))
                .map(Run::from_output);
                let native = run(Command::new(&executable).current_dir(&dir)).map(Run::from_output);
                match (interpreted, native) {
                    (Ok(interpreted), Ok(native)) => {
                        if let Err(err) = compare_runs(&interpreted, &native) {
                            failures.push(format!("{name}: {err}"));
                        }
                    }
                    (interpreted, native) => {
                        for err in [interpreted.err(), native.err()].into_iter().flatten() {
                            failures.push(format!("{name}: {err}"));
                        }
                    }
                }
            }
        }
    }

    let _ = fs::remove_dir_all(&root);
    assert!(
        failures.is_empty(),
        "{} fixture(s) failed native parity:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// The comparison above must actually reject a wrong answer: a compiled
/// program that prints `hi {x}` where the interpreter prints `hi 2.5` is the
/// exact silent divergence this harness exists to catch.
#[test]
fn a_silent_output_difference_is_a_failure() {
    let interpreted = Run {
        code: Some(0),
        stdout: "hi 2.5\n".to_string(),
        stderr: String::new(),
    };
    let native = Run {
        code: Some(0),
        stdout: "hi {x}\n".to_string(),
        stderr: String::new(),
    };
    assert!(compare_runs(&interpreted, &native).is_err());
}

/// A crash inside the compiler is not an honest refusal, even if it happens to
/// exit non-zero.
#[test]
fn a_compiler_panic_is_not_a_refusal() {
    let stderr = "\nthread 'main' (1) panicked at src/backend/x86_64/gen.rs:461:57:\nno entry found for key\n";
    assert!(check_refusal(stderr, &["E177".to_string()]).is_err());
}
