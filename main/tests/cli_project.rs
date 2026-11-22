//! Integration tests for `akk new` and for the `akk run` project header.
//!
//! Each test works in a fresh folder under the system temp directory, sets
//! `NO_COLOR=1` and captures both pipes, so it always takes the non-terminal
//! path: one static ASCII `Building` line, no redraw.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("akk-{}-{}", tag, std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn akk(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_akk"))
        .args(args)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .output()
        .expect("failed to spawn the akk binary")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace("\r\n", "\n")
}

/// Drops a `Compiled in ...` line, the way `tests/cli.rs`
/// does, so a stdout comparison sees only what the program printed.
fn without_timing_line(stdout: String) -> String {
    let mut lines: Vec<&str> = stdout.lines().collect();
    lines.retain(|line| !line.starts_with("Compiled in "));
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

fn write_main_akk(project: &Path, body: &str) {
    fs::write(project.join("src").join("main.akk"), body).expect("write main.akk");
}

fn new_demo(dir: &Path) -> PathBuf {
    let out = akk(dir, &["new", "demo"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", text(&out.stderr));
    dir.join("demo")
}

fn assert_dir_empty(dir: &Path) {
    let entries: Vec<_> = fs::read_dir(dir)
        .expect("read temp dir")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name())
        .collect();
    assert!(entries.is_empty(), "unexpected entries: {entries:?}");
}

#[test]
fn new_creates_tree_and_contents() {
    let dir = temp_dir("new-tree");
    let out = akk(&dir, &["new", "demo"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(text(&out.stdout).contains("akk new: \"demo\""));
    let project = dir.join("demo");
    assert!(project.join("akk.toml").is_file());
    assert!(project.join(".gitignore").is_file());
    assert!(project.join("src").join("main.akk").is_file());

    let manifest = fs::read_to_string(project.join("akk.toml")).unwrap();
    assert!(manifest.contains("name = \"demo\""));
    assert!(manifest.contains("version = \"0.1.0\""));
    assert!(manifest.contains("entry = \"src/main.akk\""));
    let ignore = fs::read_to_string(project.join(".gitignore")).unwrap();
    assert!(ignore.contains("/build/"));
    assert!(ignore.contains("*.akop"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn new_existing_folder_fails() {
    let dir = temp_dir("new-exists");
    fs::create_dir_all(dir.join("demo")).unwrap();
    let out = akk(&dir, &["new", "demo"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("E166"));
    // The pre-existing folder is untouched: still empty.
    assert_eq!(fs::read_dir(dir.join("demo")).unwrap().count(), 0);
    let _ = fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn new_dangling_symlink_is_refused() {
    let dir = temp_dir("new-symlink");
    let link = dir.join("demo");
    std::os::unix::fs::symlink(dir.join("does-not-exist"), &link).expect("symlink");
    // `Path::exists` is false for a dangling link; `symlink_metadata` is not.
    assert!(!link.exists());

    let out = akk(&dir, &["new", "demo"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("E166"));
    // The link is still there and still points nowhere.
    assert!(link.symlink_metadata().is_ok());
    assert!(!link.exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn new_invalid_names() {
    let dir = temp_dir("new-invalid");
    for name in ["", "..", "a/b", "a\\b", "-x", "my app", "CON", "a:b"] {
        let out = akk(&dir, &["new", name]);
        let stderr = text(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "name {name:?} stderr {stderr}");
        if name.is_empty() {
            // `args.get(2)` yields `Some("")`; cmd_new treats it as missing.
            assert!(stderr.contains("Usage: akk new <name>"), "{stderr}");
        } else {
            assert!(stderr.contains("E167"), "name {name:?} stderr {stderr}");
        }
        // `dir.join("")` is `dir` itself and `dir.join("..")` is the parent,
        // so the only safe check is that the working directory stayed empty.
        assert_dir_empty(&dir);
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn new_missing_name_prints_usage() {
    let dir = temp_dir("new-usage");
    let out = akk(&dir, &["new"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("Usage: akk new <name>"));
    assert_dir_empty(&dir);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn run_without_manifest_frozen_error() {
    let dir = temp_dir("run-no-manifest");
    // `manifest::discover` walks every parent, so a manifest anywhere above the
    // temp directory would change the expected message: skip, do not fail.
    if dir.ancestors().any(|p| p.join("akk.toml").is_file()) {
        eprintln!(
            "skipping run_without_manifest_frozen_error: an akk.toml sits above {}",
            dir.display()
        );
        let _ = fs::remove_dir_all(&dir);
        return;
    }

    let out = akk(&dir, &["run"]);
    let stderr = text(&out.stderr);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stdout), "", "stdout must stay empty");
    assert!(stderr.starts_with("error: akk.toml ကို `"), "{stderr}");
    assert!(stderr.trim_end().ends_with("` ၌ မတွေ့ပါ။"), "{stderr}");

    let reported = stderr
        .trim_end()
        .trim_start_matches("error: akk.toml ကို `")
        .trim_end_matches("` ၌ မတွေ့ပါ။")
        .to_string();
    let shown = reported.strip_prefix(r"\\?\").unwrap_or(&reported);
    // On macOS the temp path can be reached through a symlinked `/var`.
    let wanted: Vec<String> = vec![
        dir.display().to_string(),
        fs::canonicalize(&dir).unwrap().display().to_string(),
    ];
    assert!(
        wanted
            .iter()
            .any(|w| w.trim_start_matches(r"\\?\") == shown),
        "reported path {shown:?} is not one of {wanted:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn new_then_run_prints_only_program_output() {
    let dir = temp_dir("run-new");
    let project = new_demo(&dir);
    let out = akk(&project, &["run"]);
    let stderr = text(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "stderr: {stderr}");
    assert_eq!(without_timing_line(text(&out.stdout)), "မင်္ဂလာပါ!\n");
    let compiling = stderr
        .find("Compiling demo 0.1.0 (")
        .expect("Compiling line");
    let building = stderr
        .find("Building [##########] 100% (3/3)")
        .expect("Building line");
    let running = stderr.find("Running").expect("Running line");
    assert!(
        compiling < building && building < running,
        "order: {stderr}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn run_project_runtime_error() {
    let dir = temp_dir("run-error");
    let project = new_demo(&dir);
    // This call is E031: `tests/cli.rs`'s runtime_uncaught case pins that code.
    write_main_akk(&project, "\"before\" ကို ပြပါ။\nnosuchfunction ကို လုပ်ပါ။\n");
    let out = akk(&project, &["run"]);
    let stderr = text(&out.stderr);
    assert_eq!(out.status.code(), Some(1));
    assert!(without_timing_line(text(&out.stdout)).contains("before"));
    let running = stderr.find("Running").expect("Running line");
    let error = stderr.find("E031").expect("E031 in stderr");
    assert!(running < error, "error must follow Running: {stderr}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn run_project_warning() {
    let dir = temp_dir("run-warn");
    let project = new_demo(&dir);
    // Assigned but never read is W001 (`warnings.rs` doc table).
    write_main_akk(&project, "x သည် 1 ဖြစ်၏။\n\"ok\" ကို ပြပါ။\n");
    let out = akk(&project, &["run"]);
    let stderr = text(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "stderr: {stderr}");
    let compiling = stderr.find("Compiling demo").expect("Compiling line");
    let building = stderr.find("Building").expect("Building line");
    let warning = stderr.find("W001").expect("W001 in stderr");
    let running = stderr.find("Running").expect("Running line");
    assert!(
        compiling < building && building < warning && warning < running,
        "order: {stderr}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn run_project_parse_error() {
    let dir = temp_dir("run-parse-error");
    let project = new_demo(&dir);
    // A statement without its '။' is E002 (the parse_missing_period case).
    write_main_akk(&project, "\"hi\" ကို ဖော်ပြပါ\n");
    let out = akk(&project, &["run"]);
    let stderr = text(&out.stderr);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr.contains("Compiling demo"), "stderr: {stderr}");
    assert!(stderr.contains("E002"), "stderr: {stderr}");
    assert!(!stderr.contains("Building"), "no Building on parse failure");
    assert!(!stderr.contains("Running"), "no Running on parse failure");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn non_tty_bar_is_static() {
    let dir = temp_dir("run-bar");
    let project = new_demo(&dir);
    let out = akk(&project, &["run"]);
    let stderr = text(&out.stderr);
    assert_eq!(
        stderr
            .lines()
            .filter(|line| *line == "Building [##########] 100% (3/3)")
            .count(),
        1,
        "stderr: {stderr}"
    );
    assert!(!stderr.contains('\r'), "no redraw when piped: {stderr}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn explicit_file_has_no_header() {
    let dir = temp_dir("run-explicit");
    let project = new_demo(&dir);
    let out = akk(&project, &["run", "src/main.akk"]);
    let stderr = text(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "stderr: {stderr}");
    assert!(!stderr.contains("Compiling"), "stderr: {stderr}");
    assert!(!stderr.contains("Building"), "stderr: {stderr}");
    assert!(!stderr.contains("Running"), "stderr: {stderr}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn run_reuses_the_compiled_file_until_the_source_changes() {
    let dir = temp_dir("run-cache");
    let project = new_demo(&dir);
    let exe = project
        .join("src")
        .join("build")
        .join(format!("main{}", std::env::consts::EXE_SUFFIX));

    // The first run compiles: it prints `Compiled in ...` and leaves the
    // executable in the `build` folder beside the source.
    let first = akk(&project, &["run", "src/main.akk"]);
    assert_eq!(
        first.status.code(),
        Some(0),
        "stderr: {}",
        text(&first.stderr)
    );
    assert!(
        text(&first.stdout).contains("Compiled in "),
        "stdout: {}",
        text(&first.stdout)
    );
    assert!(exe.is_file(), "missing {}", exe.display());
    let first_out = without_timing_line(text(&first.stdout));

    // The second run is the executable itself: same output, no compile line.
    let second = akk(&project, &["run", "src/main.akk"]);
    assert_eq!(
        second.status.code(),
        Some(0),
        "stderr: {}",
        text(&second.stderr)
    );
    assert!(
        !text(&second.stdout).contains("Compiled in "),
        "stdout: {}",
        text(&second.stdout)
    );
    assert_eq!(without_timing_line(text(&second.stdout)), first_out);

    // A changed source is compiled again.
    std::thread::sleep(std::time::Duration::from_millis(20));
    write_main_akk(&project, "\"changed\" ကို ပြပါ။\n");
    let third = akk(&project, &["run", "src/main.akk"]);
    assert_eq!(
        third.status.code(),
        Some(0),
        "stderr: {}",
        text(&third.stderr)
    );
    assert!(
        text(&third.stdout).contains("Compiled in "),
        "stdout: {}",
        text(&third.stdout)
    );
    assert_eq!(without_timing_line(text(&third.stdout)), "changed\n");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn plain_file_run_unchanged() {
    let dir = temp_dir("run-plain");
    fs::write(dir.join("hello.akk"), "\"Hello\" ကို ပြပါ။\n").unwrap();
    let out = akk(&dir, &["hello.akk"]);
    let stderr = text(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "stderr: {stderr}");
    assert_eq!(without_timing_line(text(&out.stdout)), "Hello\n");
    assert_eq!(stderr, "", "plain runs print no header");
    let _ = fs::remove_dir_all(&dir);
}
