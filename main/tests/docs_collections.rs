//! Run every sample in `docs/COLLECTIONS.md` with `akk` and compare its output.
//!
//! A sample is a fenced block whose info string is exactly `akk` (run the
//! interpreter) or `akk-build` (run `akk build`). Its expected result lives
//! inside the same fence: after a `# ရလဒ်:` line one `# `-prefixed line per
//! stdout line, and after a `# အမှား:` line one `# `-prefixed line per stderr
//! line (a sample with an expected error must exit 1 and print nothing on
//! stdout). Both markers are ordinary Akkhara comments, so the fence text
//! itself is always a valid program — the line numbers quoted in a message are
//! the line numbers of the block as written in the page.
//!
//! A fence without a marker fails the test, so no sample can be added to the
//! page without being verified.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Sample {
    /// 1-based order on the page, used in failure messages.
    index: usize,
    /// The line the fence opens on, for failure messages.
    line: usize,
    /// `akk build` instead of `akk`.
    build: bool,
    /// The whole fence, markers included (they are comments to Akkhara).
    program: String,
    stdout: String,
    /// `Some` when the sample must fail with this stderr.
    stderr: Option<String>,
}

fn page() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("docs")
        .join("COLLECTIONS.md")
}

fn akk() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_akk"))
}

fn strip_comment(line: &str) -> &str {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix("# ") {
        rest
    } else if trimmed == "#" {
        ""
    } else {
        line
    }
}

/// Every runnable fence of the page, in order.
fn samples(markdown: &str) -> Vec<Sample> {
    let mut samples = Vec::new();
    let mut lines = markdown.lines().enumerate();
    while let Some((number, line)) = lines.next() {
        let fence = line.trim();
        if fence != "```akk" && fence != "```akk-build" {
            continue;
        }
        let build = fence == "```akk-build";
        let mut program = String::new();
        let mut stdout = String::new();
        let mut stderr: Option<String> = None;
        // 0 = program, 1 = stdout, 2 = stderr.
        let mut part = 0u8;
        loop {
            let (_, body) = lines
                .next()
                .unwrap_or_else(|| panic!("the fence at line {} is never closed", number + 1));
            if body.trim() == "```" {
                break;
            }
            match body.trim() {
                "# ရလဒ်:" => {
                    part = 1;
                    continue;
                }
                "# အမှား:" => {
                    part = 2;
                    stderr = Some(String::new());
                    continue;
                }
                _ => {}
            }
            match part {
                0 => {
                    program.push_str(body);
                    program.push('\n');
                }
                1 => {
                    stdout.push_str(strip_comment(body));
                    stdout.push('\n');
                }
                _ => {
                    let stderr = stderr.as_mut().expect("the marker set it");
                    stderr.push_str(strip_comment(body));
                    stderr.push('\n');
                }
            }
        }
        samples.push(Sample {
            index: samples.len() + 1,
            line: number + 1,
            build,
            program,
            stdout,
            stderr,
        });
    }
    samples
}

fn run(command: &mut Command) -> Output {
    command
        .output()
        .unwrap_or_else(|error| panic!("failed to run {:?}: {error}", command))
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace("\r\n", "\n")
}

#[test]
fn every_documented_collection_sample_matches_the_interpreter() {
    let markdown = fs::read_to_string(page()).expect("docs/COLLECTIONS.md");
    let samples = samples(&markdown);
    assert!(
        samples.len() >= 16,
        "expected at least 16 runnable samples in docs/COLLECTIONS.md, found {}",
        samples.len()
    );

    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("docs-tests");
    let _ = fs::remove_dir_all(&root);

    let mut failures = Vec::new();
    for sample in &samples {
        let name = format!("sample {} (line {})", sample.index, sample.line);
        if sample.stdout.trim().is_empty() && sample.stderr.is_none() {
            failures.push(format!("{name}: no `# ရလဒ်:` or `# အမှား:` marker"));
            continue;
        }
        let dir = root.join(format!("sample-{}", sample.index));
        fs::create_dir_all(&dir).expect("a work directory");
        let source = dir.join("sample.akk");
        fs::write(&source, &sample.program).expect("the sample");

        let output = if sample.build {
            run(Command::new(akk()).arg("build").arg(&source))
        } else {
            run(Command::new(akk()).arg(&source))
        };
        let actual_stdout = text(&output.stdout);
        let actual_stderr = text(&output.stderr);
        let want_stdout = sample.stdout.trim_end();
        let want_stderr = sample.stderr.as_deref().map(str::trim_end);

        if actual_stdout.trim_end() != want_stdout {
            failures.push(format!(
                "{name}: stdout was {:?}, expected {:?}",
                actual_stdout.trim_end(),
                want_stdout
            ));
        }
        match want_stderr {
            Some(want) => {
                if output.status.code() != Some(1) {
                    failures.push(format!("{name}: expected exit 1, got {}", output.status));
                }
                if actual_stderr.trim_end() != want {
                    failures.push(format!(
                        "{name}: stderr was {:?}, expected {:?}",
                        actual_stderr.trim_end(),
                        want
                    ));
                }
            }
            None => {
                if !output.status.success() {
                    failures.push(format!(
                        "{name}: expected success, got {} with stderr {:?}",
                        output.status,
                        actual_stderr.trim_end()
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} documented sample(s) did not match:\\n\\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// The page has to be reachable from the README's language reference.
#[test]
fn the_collections_page_is_linked_from_the_readme() {
    let readme = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md"))
        .expect("README.md");
    assert!(
        readme.contains("docs/COLLECTIONS.md"),
        "README.md should link docs/COLLECTIONS.md"
    );
    assert!(page().exists(), "docs/COLLECTIONS.md should exist");
}
