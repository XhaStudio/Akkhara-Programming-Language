//! Integration tests for `akk build` and for the file it writes.
//!
//! Every test works in a fresh folder under the system temp directory, sets
//! `NO_COLOR=1` and captures both pipes, so it always takes the non-terminal
//! path. A built file is a copy of the test's own `akk` binary with the
//! program appended, so the tests run it directly and compare it with
//! `akk run <entry>`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const SLOT_MAGIC: &[u8] = b"AKKSLOT1";
const HEADER_MAGIC: &[u8] = b"AKKBNDL1";

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("akk-bundle-{}-{}", tag, std::process::id()));
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

fn run_exe(exe: &Path, cwd: &Path, args: &[&str]) -> Output {
    Command::new(exe)
        .args(args)
        .current_dir(cwd)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .output()
        .expect("failed to run the built executable")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace("\r\n", "\n")
}

/// Drops a `Compiled in ...` line, the way `tests/cli.rs` does, so a stdout
/// comparison sees only what the program printed.
fn without_timing_line(stdout: String) -> String {
    let lines: Vec<&str> = stdout
        .lines()
        .filter(|line| !line.starts_with("Compiled in "))
        .collect();
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

fn demo(dir: &Path) -> PathBuf {
    let out = akk(dir, &["new", "demo"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", text(&out.stderr));
    dir.join("demo")
}

fn write_main(project: &Path, body: &str) {
    fs::write(project.join("src").join("main.akk"), body).expect("write main.akk");
}

fn write_script(project: &Path, name: &str, body: &str) {
    fs::write(project.join("src").join(name), body).expect("write script");
}

/// The manifest `akk new` writes is the base; each test adds what it needs.
fn write_manifest(project: &Path, extra_build: &str, extra_tables: &str) {
    let manifest = format!(
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nentry = \"src/main.akk\"\n\n[build]\nout_dir = \"build\"\n{}\n[run]\ntranscript = true\nwarnings = true\ntiming = true\n{}",
        extra_build, extra_tables
    );
    fs::write(project.join("akk.toml"), manifest).expect("write akk.toml");
}

fn built_exe(project: &Path) -> PathBuf {
    project
        .join("build")
        .join(format!("demo{}", std::env::consts::EXE_SUFFIX))
}

fn build(project: &Path) -> Output {
    akk(project, &["build"])
}

fn assert_built(project: &Path) -> PathBuf {
    let out = build(project);
    assert_eq!(
        out.status.code(),
        Some(0),
        "build failed: {}",
        text(&out.stderr)
    );
    let exe = built_exe(project);
    assert!(exe.is_file(), "missing {}", exe.display());
    exe
}

fn count_magic(bytes: &[u8], magic: &[u8]) -> usize {
    if bytes.len() < magic.len() {
        return 0;
    }
    (0..=bytes.len() - magic.len())
        .filter(|at| &bytes[*at..at + magic.len()] == magic)
        .count()
}

/// CRC-32/IEEE, the one the bundle uses. The integration test cannot reach the
/// binary's private module, so the reference implementation is repeated here.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// The header offset of a built file: `len - trailer - body_len`.
fn body_offset(bytes: &[u8]) -> usize {
    let len = bytes.len();
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&bytes[len - 24 + 8..len - 24 + 16]);
    let body_len = u64::from_le_bytes(raw) as usize;
    len - 24 - body_len
}

#[test]
fn build_creates_exe_and_prints_finished() {
    let dir = temp_dir("build-finished");
    let project = demo(&dir);
    write_main(&project, "\"hi\" ကို ပြပါ။\n");

    let out = build(&project);
    let stderr = text(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr);

    let exe = built_exe(&project);
    assert!(exe.is_file(), "missing {}", exe.display());
    let stdout = text(&out.stdout);
    assert!(
        stdout.trim_end().starts_with("Compiled in ") && stdout.lines().count() == 1,
        "stdout: {stdout}"
    );

    let compiling = stderr.find("Compiling").expect("Compiling line");
    let building = stderr.find("Building").expect("Building line");
    let finished = stderr.find("Finished").expect("Finished line");
    assert!(
        compiling < building && building < finished,
        "order: {}",
        stderr
    );
    assert!(stderr.contains("Finished"));
    assert!(stderr.contains("bytes)"));

    // `akk build` runs nothing and writes no transcript.
    assert!(!exe.with_extension("akop").exists());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn build_skips_work_when_the_output_is_current() {
    let dir = temp_dir("build-cache");
    let project = demo(&dir);
    write_main(&project, "\"hi\" ကို ပြပါ။\n");
    let exe = assert_built(&project);
    let first = fs::read(&exe).unwrap();

    // A second build over unchanged inputs writes nothing.
    let again = build(&project);
    assert_eq!(again.status.code(), Some(0));
    assert!(
        text(&again.stdout).starts_with("Already compiled "),
        "stdout: {}",
        text(&again.stdout)
    );
    assert_eq!(fs::read(&exe).unwrap(), first);

    // A newer source is compiled again.
    std::thread::sleep(std::time::Duration::from_millis(20));
    write_main(&project, "\"changed\" ကို ပြပါ။\n");
    let rebuilt = build(&project);
    assert_eq!(
        rebuilt.status.code(),
        Some(0),
        "stderr: {}",
        text(&rebuilt.stderr)
    );
    assert!(
        text(&rebuilt.stdout).contains("Compiled in "),
        "stdout: {}",
        text(&rebuilt.stdout)
    );
    assert_ne!(fs::read(&exe).unwrap(), first);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn built_exe_stdout_matches_run() {
    let dir = temp_dir("parity-stdout");
    let project = demo(&dir);
    // Prints, and leaves one variable assigned-but-never-read for W001.
    write_main(&project, "x သည် 1 ဖြစ်၏။\n\"ok\" ကို ပြပါ။\n");
    let exe = assert_built(&project);

    let run = akk(&project, &["run", "src/main.akk"]);
    let built = run_exe(&exe, &project, &[]);

    assert_eq!(built.status.code(), run.status.code());
    assert_eq!(
        without_timing_line(text(&built.stdout)),
        without_timing_line(text(&run.stdout))
    );
    // The program's own warnings carry no file prefix, so stderr matches too.
    assert_eq!(text(&built.stderr), text(&run.stderr));
    assert!(text(&built.stderr).contains("W001"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn built_exe_runtime_error_parity() {
    let dir = temp_dir("parity-error");
    let project = demo(&dir);
    // E031: calling a function that does not exist.
    write_main(&project, "\"before\" ကို ပြပါ။\nnosuchfunction ကို လုပ်ပါ။\n");
    let exe = assert_built(&project);

    let run = akk(&project, &["run", "src/main.akk"]);
    let built = run_exe(&exe, &project, &[]);

    assert_eq!(built.status.code(), Some(1));
    assert_eq!(built.status.code(), run.status.code());
    assert_eq!(text(&built.stderr), text(&run.stderr));
    assert!(text(&built.stderr).contains("E031"));

    // The transcript is written beside the executable, with the error in it.
    let log = exe.with_extension("akop");
    assert!(log.is_file(), "missing {}", log.display());
    assert!(text(&fs::read(&log).unwrap()).contains("E031"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn akop_beside_exe_named_after_exe_stem() {
    let dir = temp_dir("akop-name");
    let project = demo(&dir);
    write_main(&project, "\"hi\" ကို ပြပါ။\n");
    let exe = assert_built(&project);

    let out = run_exe(&exe, &project, &[]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", text(&out.stderr));

    let log = project.join("build").join("demo.akop");
    assert!(log.is_file(), "missing {}", log.display());
    let saved = text(&fs::read(&log).unwrap());
    assert_eq!(saved, "hi\n", "log: {}", saved);
    assert!(!saved.contains("Interpreted in ") && !saved.contains("Compiled in "));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn built_exe_prints_no_timer() {
    let dir = temp_dir("timing");
    let project = demo(&dir);
    write_main(&project, "\"hi\" ကို ပြပါ။\n");

    // A built file prints no timer: the compile time belongs to the build
    // that produced it, not to the program's own output.
    let exe = assert_built(&project);
    let on = text(&run_exe(&exe, &project, &[]).stdout);
    assert_eq!(on.trim_end(), "hi", "stdout: {}", on);
    assert!(!on.contains("Interpreted in ") && !on.contains("Compiled in "));

    // The old `[run] timing = false` key still parses, and still adds nothing.
    write_manifest(&project, "", "");
    let manifest = fs::read_to_string(project.join("akk.toml")).unwrap();
    fs::write(
        project.join("akk.toml"),
        manifest.replace("timing = true", "timing = false"),
    )
    .unwrap();
    let exe = assert_built(&project);
    let off = text(&run_exe(&exe, &project, &[]).stdout);
    assert_eq!(off.trim_end(), "hi", "stdout: {}", off);
    assert!(!off.contains("Interpreted in ") && !off.contains("Compiled in "));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bundle_ignores_argv() {
    let dir = temp_dir("argv");
    let project = demo(&dir);
    write_main(&project, "\"program\" ကို ပြပါ။\n");
    let exe = assert_built(&project);

    for args in [
        vec!["update"],
        vec!["build"],
        vec!["--version"],
        vec!["help"],
        vec!["x.akk"],
        vec!["--no-warn"],
    ] {
        let out = run_exe(&exe, &project, &args);
        let stdout = text(&out.stdout);
        assert_eq!(out.status.code(), Some(0), "args {:?}", args);
        assert!(stdout.contains("program"), "args {:?}: {}", args, stdout);
        assert!(!stdout.contains("akk "), "args {:?}: {}", args, stdout);
        assert!(!stdout.contains("Usage:"), "args {:?}: {}", args, stdout);
    }

    // No libraries/ folder is created beside a built file.
    assert!(!project.join("build").join("libraries").exists());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn embedded_script_wins_over_cwd() {
    let dir = temp_dir("embedded-wins");
    let project = demo(&dir);
    write_main(&project, "နည်းပညာများ Tools ကို သုံးမည်။\n\"main\" ကို ပြပါ။\n");
    write_script(&project, "Tools.akk", "\"embedded\" ကို ပြပါ။\n");
    let exe = assert_built(&project);

    // A different Tools.akk in the folder the executable is run from.
    let elsewhere = dir.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::write(elsewhere.join("Tools.akk"), "\"cwd\" ကို ပြပါ။\n").unwrap();

    let out = run_exe(&exe, &elsewhere, &[]);
    let stdout = text(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", text(&out.stderr));
    assert!(stdout.contains("embedded"), "stdout: {}", stdout);
    assert!(!stdout.contains("cwd"), "stdout: {}", stdout);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn dependency_path_embedded() {
    let dir = temp_dir("dependency");
    let project = demo(&dir);
    write_manifest(
        &project,
        "",
        "\n[dependencies]\nTools = { path = \"vendor/Tools.akk\" }\n",
    );
    write_main(&project, "နည်းပညာများ Tools ကို သုံးမည်။\n\"main\" ကို ပြပါ။\n");
    fs::create_dir_all(project.join("vendor")).unwrap();
    fs::write(
        project.join("vendor").join("Tools.akk"),
        "\"vendored\" ကို ပြပါ။\n",
    )
    .unwrap();

    let exe = assert_built(&project);
    // The original script is gone; the built file still has it.
    fs::remove_file(project.join("vendor").join("Tools.akk")).unwrap();

    let out = run_exe(&exe, &project, &[]);
    let stdout = text(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", text(&out.stderr));
    assert!(stdout.contains("vendored"), "stdout: {}", stdout);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn unresolved_use_is_runtime_error() {
    let dir = temp_dir("unresolved");
    let project = demo(&dir);
    write_main(&project, "နည်းပညာများ Nowhere ကို သုံးမည်။\n\"after\" ကို ပြပါ။\n");
    // Building succeeds: an unresolvable name is a run-time problem, as it is
    // for `akk run`.
    let exe = assert_built(&project);

    let built = run_exe(&exe, &project, &[]);
    let run = akk(&project, &["run", "src/main.akk"]);
    assert_eq!(built.status.code(), Some(1));
    assert_eq!(built.status.code(), run.status.code());
    assert_eq!(text(&built.stderr), text(&run.stderr));
    assert!(!text(&built.stdout).contains("after"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn truncated_exe_fails_e159() {
    let dir = temp_dir("truncated");
    let project = demo(&dir);
    write_main(&project, "\"hi\" ကို ပြပါ။\n");
    let exe = assert_built(&project);

    let mut bytes = fs::read(&exe).unwrap();
    bytes.truncate(bytes.len() - 30);
    fs::write(&exe, &bytes).unwrap();

    let out = run_exe(&exe, &project, &[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("E159"));
    assert!(!text(&out.stdout).contains("hi"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn corrupted_payload_fails_e159() {
    let dir = temp_dir("corrupted");
    let project = demo(&dir);
    write_main(&project, "\"hi\" ကို ပြပါ။\n");
    let exe = assert_built(&project);

    let mut bytes = fs::read(&exe).unwrap();
    let at = body_offset(&bytes) + 40 + 1; // one byte into the source
    bytes[at] ^= 0xFF;
    fs::write(&exe, &bytes).unwrap();

    let out = run_exe(&exe, &project, &[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("E159"));
    assert!(!text(&out.stdout).contains("hi"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn unknown_version_fails_e160() {
    let dir = temp_dir("version");
    let project = demo(&dir);
    write_main(&project, "\"hi\" ကို ပြပါ။\n");
    let exe = assert_built(&project);

    let mut bytes = fs::read(&exe).unwrap();
    let header = body_offset(&bytes);
    bytes[header + 8..header + 10].copy_from_slice(&9u16.to_le_bytes());
    // Keep the body CRC valid so the version is what fails, not the checksum.
    let body = bytes[header..bytes.len() - 24].to_vec();
    let crc = crc32(&body).to_le_bytes();
    let len = bytes.len();
    bytes[len - 24 + 16..len - 24 + 20].copy_from_slice(&crc);
    fs::write(&exe, &bytes).unwrap();

    let out = run_exe(&exe, &project, &[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("E160"), "{}", text(&out.stderr));
    assert!(!text(&out.stdout).contains("hi"));

    let _ = fs::remove_dir_all(&dir);
}

/// A bytecode artifact written by an earlier revision is refused with `E175`,
/// naming both revisions, instead of being read with the wrong instruction
/// layout. The payload's revision field is patched and both checksums are
/// recomputed, so the version -- not a checksum -- is what fails, exactly as a
/// stale artifact from an older `akk` would.
#[test]
fn stale_bytecode_artifact_fails_e175() {
    let dir = temp_dir("bytecode-version");
    let project = demo(&dir);
    write_main(&project, "\"hi\" ကို ပြပါ။\n");
    write_manifest(&project, "mode = \"bytecode\"\n", "");
    let exe = assert_built(&project);

    let mut bytes = fs::read(&exe).unwrap();
    let header = body_offset(&bytes);
    let payload = header + 40; // bundle header, then the compiled program
    assert_eq!(&bytes[payload..payload + 8], b"AKKBC001");

    let mut raw = [0u8; 8];
    raw.copy_from_slice(&bytes[header + 16..header + 24]);
    let payload_len = u64::from_le_bytes(raw) as usize;
    bytes[payload + 8..payload + 10].copy_from_slice(&14u16.to_le_bytes());

    let crc = crc32(&bytes[payload..payload + payload_len]).to_le_bytes();
    bytes[header + 32..header + 36].copy_from_slice(&crc);
    let len = bytes.len();
    let body_crc = crc32(&bytes[header..len - 24]).to_le_bytes();
    bytes[len - 24 + 16..len - 24 + 20].copy_from_slice(&body_crc);
    fs::write(&exe, &bytes).unwrap();

    let out = run_exe(&exe, &project, &[]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("E175"), "stderr: {stderr}");
    assert!(stderr.contains("14"), "stderr: {stderr}");
    assert!(!text(&out.stdout).contains("hi"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn slot_is_unique_in_built_exe() {
    let dir = temp_dir("slot-unique");
    let project = demo(&dir);
    write_main(&project, "\"hi\" ကို ပြပါ။\n");
    let exe = assert_built(&project);

    let built = fs::read(&exe).unwrap();
    assert_eq!(count_magic(&built, SLOT_MAGIC), 1);

    let plain = fs::read(env!("CARGO_BIN_EXE_akk")).unwrap();
    assert_eq!(count_magic(&plain, SLOT_MAGIC), 1);

    // Exactly one appended body: the built file is the interpreter image with
    // the body after it. The header magic also occurs once inside that image's
    // own data, so the count starts past it. (The image itself is not
    // byte-identical to the plain one -- the slot inside it is patched.)
    assert_eq!(
        count_magic(&built[plain.len()..], HEADER_MAGIC),
        1,
        "one header in the {} bytes after the image",
        built.len() - plain.len()
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn build_without_manifest_prints_frozen_error() {
    let dir = temp_dir("no-manifest");
    let out = build(&dir);
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("error: akk.toml ကို `"), "stderr: {}", stderr);
    assert!(stderr.contains("မတွေ့ပါ။"), "stderr: {}", stderr);
    assert!(!stderr.contains("E1"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn build_rejects_include_e164() {
    let dir = temp_dir("include");
    let project = demo(&dir);
    write_main(&project, "\"hi\" ကို ပြပါ။\n");
    write_manifest(&project, "\n[build.bundle]\ninclude = [\"assets\"]\n", "");

    let out = build(&project);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("E164"), "{}", text(&out.stderr));
    assert!(!built_exe(&project).exists());

    let _ = fs::remove_dir_all(&dir);
}

/// True when `needle` occurs anywhere in `bytes`.
fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|window| window == needle)
}

/// Only the error lines of a diagnostic stream: the interpreter and the VM
/// differ in warnings (only the interpreter's front end emits `W0xx`), so the
/// comparison is about the failure itself.
fn errors_only(stderr: &str) -> String {
    stderr
        .lines()
        .filter(|line| !line.starts_with("W0"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The program's own output, without a `Compiled in ...` line.
fn without_timing(stdout: &str) -> String {
    stdout
        .lines()
        .filter(|line| !line.starts_with("Compiled in "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The constructs the mission lists, one program: both variable spellings,
/// if / else-if / else, both while forms, both for forms, and a function
/// defined, returning a value and called.
const EVERY_CONSTRUCT: &str = "# SOURCE_ONLY_SENTINEL_987\n\
     a က 1။\n\
     b သည် 2 ဖြစ်၏။\n\
     x က 10။\n\
     a ကို ဖော်ပြပါ။\n\
     b ကို ဖော်ပြပါ။\n\
     (x > 10) ဖြစ်ရင်\n    \"pos\" ကို ပြပါ။\n\
     အခြား (x == 10) မဖြစ်ရင်\n    \"neg-elif\" ကို ပြပါ။\n\
     မဖြစ်ရင်\n    \"else\" ကို ပြပါ။\n\
     ပြီး။\n\
     i သည် 0 ဖြစ်၏။\n\
     (i < 3) ဖြစ်နေရင်\n    i ကို ဖော်ပြပါ။\n    i သည် i + 1 ဖြစ်၏။\nပြီး။\n\
     j သည် 8 ဖြစ်၏။\n\
     (j > 9) မဖြစ်နေရင်\n    \"neg-while\" ကို ပြပါ။\n    j သည် j + 1 ဖြစ်၏။\nပြီး။\n\
     k သည် တစ်ခုစီ(0, 6, 2)\n    k ကို ဖော်ပြပါ။\nပြီး။\n\
     xs မှာ [7, 8, 9] ဖြစ်၏။\n\
     x2 သည် တစ်ခုစီ(xs)\n    x2 ကို ဖော်ပြပါ။\nပြီး။\n\
     လုပ်ငန်း double(n)သည်\n    n * 2\nပြီး။\n\
     d သည် double(21) ဖြစ်၏။\n\
     d ကို ဖော်ပြပါ။\n\
     \"Hello, world\" ကို ပြပါ။\n";

#[test]
fn build_bytecode_embeds_instructions_not_source() {
    let dir = temp_dir("bytecode");
    let project = demo(&dir);
    write_main(&project, EVERY_CONSTRUCT);
    write_manifest(&project, "mode = \"bytecode\"\n", "");

    let exe = assert_built(&project);
    let bytes = fs::read(&exe).unwrap();
    assert!(
        !contains(&bytes, b"SOURCE_ONLY_SENTINEL_987"),
        "the source text must not be embedded in a bytecode build"
    );
    assert!(
        contains(&bytes, b"AKKBC001"),
        "the bytecode container should be embedded"
    );

    let out = run_exe(&exe, &project, &[]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", text(&out.stderr));
    let from_exe = without_timing(&text(&out.stdout));
    let from_interpreter = without_timing(&text(&akk(&project, &["src/main.akk"]).stdout));
    assert_eq!(from_exe, from_interpreter);
    assert!(from_exe.contains("42"), "the function call ran: {from_exe}");

    let _ = fs::remove_dir_all(&dir);
}

/// Lambdas and closures compiled into an artifact: a lambda stored in a
/// variable, one closing over an outer variable, one returned from a call, and
/// the callback builtins. The built executable must print what `akk run` does.
const LAMBDAS: &str = "# SOURCE_ONLY_SENTINEL_3131\n\
     factor :int = 10;\n\
     scale = fn(x) { x * factor };\n\
     xs = [1, 2, 3, 4, 5];\n\
     print(map(xs, scale));\n\
     print(filter(xs, fn(x) { x % 2 == 0 }));\n\
     print(reduce(xs, (a, b) => a + b, 0));\n\
     adder = fn(n) { fn(x) { x + n } };\n\
     add5 = adder(5);\n\
     print(add5(10));\n";

#[test]
fn bytecode_build_runs_lambdas_and_closures() {
    let dir = temp_dir("bytecode-lambdas");
    let project = demo(&dir);
    write_main(&project, LAMBDAS);
    write_manifest(&project, "mode = \"bytecode\"\n", "");

    let exe = assert_built(&project);
    let bytes = fs::read(&exe).unwrap();
    assert!(!contains(&bytes, b"SOURCE_ONLY_SENTINEL_3131"));
    assert!(contains(&bytes, b"AKKBC001"));

    let built = run_exe(&exe, &project, &[]);
    assert_eq!(
        built.status.code(),
        Some(0),
        "stderr: {}",
        text(&built.stderr)
    );
    let from_artifact = without_timing(&text(&built.stdout));

    let interpreted = akk(&project, &["src/main.akk"]);
    assert_eq!(
        interpreted.status.code(),
        Some(0),
        "{}",
        text(&interpreted.stderr)
    );
    let from_interpreter = without_timing(&text(&interpreted.stdout));

    let on_vm = akk(&project, &["--vm", "src/main.akk"]);
    assert_eq!(on_vm.status.code(), Some(0), "{}", text(&on_vm.stderr));
    let from_vm = without_timing(&text(&on_vm.stdout));

    assert_eq!(from_artifact, from_interpreter, "artifact vs interpreter");
    assert_eq!(from_vm, from_interpreter, "vm vs interpreter");
    assert_eq!(from_interpreter, "[10, 20, 30, 40, 50]\n[2, 4]\n15\n15");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn build_interpreter_mode_embeds_source() {
    let dir = temp_dir("interpreter-mode");
    let project = demo(&dir);
    write_main(&project, EVERY_CONSTRUCT);
    write_manifest(&project, "mode = \"interpreter\"\n", "");

    let exe = assert_built(&project);
    let bytes = fs::read(&exe).unwrap();
    assert!(
        contains(&bytes, b"SOURCE_ONLY_SENTINEL_987"),
        "the interpreter build carries the source text"
    );
    let out = run_exe(&exe, &project, &[]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", text(&out.stderr));
    let from_exe = without_timing(&text(&out.stdout));
    let from_interpreter = without_timing(&text(&akk(&project, &["src/main.akk"]).stdout));
    assert_eq!(from_exe, from_interpreter);

    let _ = fs::remove_dir_all(&dir);
}

/// An index write replaces the item that is there and creates one at the
/// position just past the end -- for lists, tuples, sets and dicts alike. The
/// bytecode machine runs the interpreter's own `set_index`, so both backends
/// must print the same thing.
#[test]
fn index_writes_create_or_replace_in_both_backends() {
    let dir = temp_dir("index-writes");
    let project = demo(&dir);
    write_main(
        &project,
        "xs မှာ [10, 20] ဖြစ်၏။\n\
         xs[0] က 99။\n\
         xs[2] က 30။\n\
         print(xs);\n\
         t မှာ (1, 2) ဖြစ်၏။\n\
         t[1] က 9။\n\
         t[2] က 3။\n\
         print(t);\n\
         s မှာ {1, 2} ဖြစ်၏။\n\
         s[0] က 7။\n\
         s[2] က 3။\n\
         print(s);\n\
         s[0] က 3။\n\
         print(s);\n\
         d မှာ { \"a\" သည် 1 ဖြစ်၏။ } ဖြစ်၏။\n\
         d[\"a\"] က 5။\n\
         d[\"b\"] က 6။\n\
         print(d);\n",
    );

    let interpreted = akk(&project, &["src/main.akk"]);
    assert_eq!(
        interpreted.status.code(),
        Some(0),
        "{}",
        text(&interpreted.stderr)
    );
    let on_vm = akk(&project, &["--vm", "src/main.akk"]);
    assert_eq!(on_vm.status.code(), Some(0), "{}", text(&on_vm.stderr));

    let from_interpreter = without_timing(&text(&interpreted.stdout));
    assert_eq!(without_timing(&text(&on_vm.stdout)), from_interpreter);
    assert_eq!(
        from_interpreter,
        "[99, 20, 30]\n(1, 9, 3)\n{7, 2, 3}\n{2, 3}\n{\"a\": 5, \"b\": 6}"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bytecode_vm_matches_the_interpreter_on_every_construct() {
    let dir = temp_dir("vm-constructs");
    let project = demo(&dir);
    write_main(&project, EVERY_CONSTRUCT);

    let interpreted = akk(&project, &["src/main.akk"]);
    assert_eq!(
        interpreted.status.code(),
        Some(0),
        "{}",
        text(&interpreted.stderr)
    );
    let on_vm = akk(&project, &["--vm", "src/main.akk"]);
    assert_eq!(on_vm.status.code(), Some(0), "{}", text(&on_vm.stderr));

    let interpreted_out = without_timing(&text(&interpreted.stdout));
    let vm_out = without_timing(&text(&on_vm.stdout));
    assert_eq!(vm_out, interpreted_out);
    // The shape of the output: both variables, the else branch, the three
    // counted loops, the function's 42, and the printed greeting.
    assert_eq!(
        interpreted_out,
        "1\n2\nelse\n0\n1\n2\nneg-while\nneg-while\n0\n2\n4\n7\n8\n9\n42\nHello, world"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// A dictionary-heavy program: literal, read by key, write by key, key
/// existence, keys iteration, a nested dictionary and printing.
const DICTIONARIES: &str = "# SOURCE_ONLY_SENTINEL_4242\n\
     d မှာ { \"a\" သည် 1 ဖြစ်၏။ \"b\" သည် 2 ဖြစ်၏။ \"c\" သည် 3 ဖြစ်၏။ } ဖြစ်၏။\n\
     d ကို ဖော်ပြပါ။\n\
     d[\"b\"] ကို ဖော်ပြပါ။\n\
     d[\"d\"] မှာ 4 ဖြစ်၏။\n\
     d ကို ဖော်ပြပါ။\n\
     (d ထဲ \"d\") ဖြစ်ရင်\n    \"has-d\" ကို ပြပါ။\nမဖြစ်ရင်\n    \"no-d\" ကို ပြပါ။\nပြီး။\n\
     (d ထဲ \"zz\") ဖြစ်ရင်\n    \"has-zz\" ကို ပြပါ။\nမဖြစ်ရင်\n    \"no-zz\" ကို ပြပါ။\nပြီး။\n\
     k သည် တစ်ခုစီ(d)\n    k ကို ဖော်ပြပါ။\nပြီး။\n\
     nested မှာ { \"inner\" သည် { \"x\" သည် 10 ဖြစ်၏။ } ဖြစ်၏။ \"n\" သည် 2 ဖြစ်၏။ } ဖြစ်၏။\n\
     nested ကို ဖော်ပြပါ။\n\
     inner သည် nested[\"inner\"] ဖြစ်၏။\n\
     inner[\"x\"] ကို ဖော်ပြပါ။\n\
     total သည် 0 ဖြစ်၏။\n\
     key သည် တစ်ခုစီ(d)\n    total သည် total + d[key] ဖြစ်၏။\nပြီး။\n\
     total ကို ဖော်ပြပါ။\n";

#[test]
fn bytecode_builds_and_runs_dictionaries() {
    let dir = temp_dir("bytecode-dicts");
    let project = demo(&dir);
    write_main(&project, DICTIONARIES);
    write_manifest(&project, "mode = \"bytecode\"\n", "");

    let exe = assert_built(&project);
    let bytes = fs::read(&exe).unwrap();
    assert!(
        !contains(&bytes, b"SOURCE_ONLY_SENTINEL_4242"),
        "the source text must not be embedded in a bytecode build"
    );
    assert!(
        contains(&bytes, b"AKKBC001"),
        "the bytecode container should be embedded"
    );

    let built = run_exe(&exe, &project, &[]);
    assert_eq!(
        built.status.code(),
        Some(0),
        "stderr: {}",
        text(&built.stderr)
    );
    let from_artifact = without_timing(&text(&built.stdout));

    let interpreted = akk(&project, &["src/main.akk"]);
    assert_eq!(
        interpreted.status.code(),
        Some(0),
        "{}",
        text(&interpreted.stderr)
    );
    let from_interpreter = without_timing(&text(&interpreted.stdout));

    let on_vm = akk(&project, &["--vm", "src/main.akk"]);
    assert_eq!(on_vm.status.code(), Some(0), "{}", text(&on_vm.stderr));
    let from_vm = without_timing(&text(&on_vm.stdout));

    assert_eq!(from_artifact, from_interpreter, "artifact vs interpreter");
    assert_eq!(from_vm, from_interpreter, "vm vs interpreter");
    assert_eq!(
        from_interpreter,
        "{\"a\": 1, \"b\": 2, \"c\": 3}\n\
         2\n\
         {\"a\": 1, \"b\": 2, \"c\": 3, \"d\": 4}\n\
         has-d\n\
         no-zz\n\
         a\n\
         b\n\
         c\n\
         d\n\
         {\"inner\": {\"x\": 10}, \"n\": 2}\n\
         10\n\
         10"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// A key that is not there fails the same way in both backends: `E053`, the
/// key printed, and a non-zero exit.
#[test]
fn a_missing_dictionary_key_fails_the_same_in_both_backends() {
    let dir = temp_dir("bytecode-dict-miss");
    let project = demo(&dir);
    write_main(
        &project,
        "d မှာ { \"a\" သည် 1 ဖြစ်၏။ } ဖြစ်၏။\nd[\"nope\"] ကို ဖော်ပြပါ။\n",
    );

    let interpreted = akk(&project, &["src/main.akk"]);
    let on_vm = akk(&project, &["--vm", "src/main.akk"]);
    assert_eq!(interpreted.status.code(), Some(1));
    assert_eq!(on_vm.status.code(), Some(1));
    assert_eq!(text(&interpreted.stderr), text(&on_vm.stderr));
    assert!(
        text(&interpreted.stderr).contains("E053"),
        "{}",
        text(&interpreted.stderr)
    );
    assert!(
        text(&interpreted.stderr).contains("nope"),
        "{}",
        text(&interpreted.stderr)
    );

    let _ = fs::remove_dir_all(&dir);
}

/// Sets and classes in one program: a set literal with a repeated member,
/// membership both ways, iteration, a class constructed with header
/// parameters, `အတု` field writes, the object printed, and both field-read
/// spellings.
const SETS_AND_CLASSES: &str = "# SOURCE_ONLY_SENTINEL_7777\n\
     s မှာ {1, 2, 2, 3} ဖြစ်၏။\n\
     s ကို ဖော်ပြပါ။\n\
     (s ထဲ 2) ဖြစ်ရင်\n    \"has2\" ကို ပြပါ။\nမဖြစ်ရင်\n    \"no2\" ကို ပြပါ။\nပြီး။\n\
     (s ထဲ 9) ဖြစ်ရင်\n    \"has9\" ကို ပြပါ။\nမဖြစ်ရင်\n    \"no9\" ကို ပြပါ။\nပြီး။\n\
     m သည် တစ်ခုစီ(s)\n    m ကို ပြပါ။\nပြီး။\n\
     ပုံသေ Point(x, y) သည်\n\
         လုပ်ငန်း Point()သည်\n        အတု px သည် x ဖြစ်၏။\n        အတု py သည် y ဖြစ်၏။\n    ပြီး။\n\
     ပြီး။\n\
     p သည် Point(3, 4) ဖြစ်၏။\n\
     p ကို ဖော်ပြပါ။\n\
     p.px ကို ပြပါ။\n\
     p ၏ py ကို ပြပါ။\n";

#[test]
fn bytecode_builds_and_runs_sets_and_classes() {
    let dir = temp_dir("bytecode-set-class");
    let project = demo(&dir);
    write_main(&project, SETS_AND_CLASSES);
    write_manifest(&project, "mode = \"bytecode\"\n", "");

    let exe = assert_built(&project);
    let bytes = fs::read(&exe).unwrap();
    assert!(
        !contains(&bytes, b"SOURCE_ONLY_SENTINEL_7777"),
        "the source text must not be embedded in a bytecode build"
    );
    assert!(
        contains(&bytes, b"AKKBC001"),
        "the bytecode container should be embedded"
    );

    let built = run_exe(&exe, &project, &[]);
    assert_eq!(
        built.status.code(),
        Some(0),
        "stderr: {}",
        text(&built.stderr)
    );
    let from_artifact = without_timing(&text(&built.stdout));

    let interpreted = akk(&project, &["src/main.akk"]);
    assert_eq!(
        interpreted.status.code(),
        Some(0),
        "{}",
        text(&interpreted.stderr)
    );
    let from_interpreter = without_timing(&text(&interpreted.stdout));

    let on_vm = akk(&project, &["--vm", "src/main.akk"]);
    assert_eq!(on_vm.status.code(), Some(0), "{}", text(&on_vm.stderr));
    let from_vm = without_timing(&text(&on_vm.stdout));

    assert_eq!(from_artifact, from_interpreter, "artifact vs interpreter");
    assert_eq!(from_vm, from_interpreter, "vm vs interpreter");
    assert_eq!(
        from_interpreter,
        "{1, 2, 3}\n\
         has2\n\
         no9\n\
         1\n\
         2\n\
         3\n\
         Point {px: 3, py: 4}\n\
         3\n\
         4"
    );

    let _ = fs::remove_dir_all(&dir);
}

/// A constructor called with the wrong number of arguments, and a field that
/// is not on the object, fail the same way in both backends.
#[test]
fn class_errors_match_in_both_backends() {
    let dir = temp_dir("bytecode-class-errors");
    let project = demo(&dir);
    write_main(
        &project,
        "ပုံသေ P(x) သည်\n\
             လုပ်ငန်း P()သည်\n        အတု a သည် x ဖြစ်၏။\n    ပြီး။\n\
         ပြီး။\n\
         p သည် P(1, 2) ဖြစ်၏။\n",
    );
    let interpreted = akk(&project, &["src/main.akk"]);
    let on_vm = akk(&project, &["--vm", "src/main.akk"]);
    assert_eq!(interpreted.status.code(), Some(1));
    assert_eq!(on_vm.status.code(), Some(1));
    assert_eq!(
        errors_only(&text(&interpreted.stderr)),
        errors_only(&text(&on_vm.stderr))
    );
    assert!(
        text(&on_vm.stderr).contains("E034"),
        "{}",
        text(&on_vm.stderr)
    );

    write_main(
        &project,
        "ပုံသေ P(x) သည်\n\
             လုပ်ငန်း P()သည်\n        အတု a သည် x ဖြစ်၏။\n    ပြီး။\n\
         ပြီး။\n\
         p သည် P(1) ဖြစ်၏။\n\
         p.zz ကို ပြပါ။\n",
    );
    let interpreted = akk(&project, &["src/main.akk"]);
    let on_vm = akk(&project, &["--vm", "src/main.akk"]);
    assert_eq!(interpreted.status.code(), Some(1));
    assert_eq!(on_vm.status.code(), Some(1));
    assert_eq!(
        errors_only(&text(&interpreted.stderr)),
        errors_only(&text(&on_vm.stderr))
    );
    assert!(
        text(&on_vm.stderr).contains("E082"),
        "{}",
        text(&on_vm.stderr)
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn build_bytecode_refuses_a_library_program() {
    let dir = temp_dir("bytecode-lib");
    let project = demo(&dir);
    write_manifest(
        &project,
        "mode = \"bytecode\"\n",
        "\n[dependencies]\nTools = { path = \"vendor/Tools.akk\" }\n",
    );
    write_main(&project, "နည်းပညာများ Tools ကို သုံးမည်။\n\"main\" ကို ပြပါ။\n");
    fs::create_dir_all(project.join("vendor")).unwrap();
    fs::write(
        project.join("vendor").join("Tools.akk"),
        "\"embedded\" ကို ပြပါ။\n",
    )
    .unwrap();

    let out = build(&project);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("E170"),
        "stderr: {}",
        text(&out.stderr)
    );
    assert!(!built_exe(&project).exists());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn build_elf_keeps_e147() {
    let dir = temp_dir("elf");
    let project = demo(&dir);
    write_main(&project, "\"hi\" ကို ပြပါ။\n");
    write_manifest(&project, "mode = \"elf\"\n", "");

    let out = build(&project);
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("E147"), "stderr: {}", stderr);
    assert!(stderr.contains("[build] mode"), "stderr: {}", stderr);
    assert!(!built_exe(&project).exists());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn build_rejects_bad_output_name() {
    let dir = temp_dir("bad-output");
    let project = demo(&dir);
    write_main(&project, "\"hi\" ကို ပြပါ။\n");

    for bad in ["../escape", "sub/demo", ""] {
        write_manifest(&project, &format!("output = \"{}\"\n", bad), "");
        let out = build(&project);
        assert_eq!(out.status.code(), Some(1), "output {:?}", bad);
        assert!(text(&out.stderr).contains("E147"), "output {:?}", bad);
    }
    assert!(!project.join("escape.exe").exists());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn plain_akk_unchanged() {
    let dir = temp_dir("plain");
    fs::write(dir.join("hello.akk"), "\"Hello\" ကို ပြပါ။\n").unwrap();

    let out = akk(&dir, &["hello.akk"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(text(&out.stderr), "");
    let stdout = text(&out.stdout);
    assert!(stdout.starts_with("Hello"), "stdout: {}", stdout);
    // No project header, because there is no manifest.
    assert!(!stdout.contains("Compiling"));
    assert!(!stdout.contains("Building"));
    assert!(!stdout.contains("Finished"));
    assert!(dir.join("hello.akop").is_file());

    let _ = fs::remove_dir_all(&dir);
}
// ---------------------------------------------------------------------------
// `input` on the bytecode backend, and the transcript it writes
// ---------------------------------------------------------------------------

/// `run_exe` with a scripted stdin: the program's reads come from `stdin`
/// instead of an interactive terminal.
fn run_exe_stdin(exe: &Path, cwd: &Path, stdin: &str) -> Output {
    use std::io::Write;
    let mut child = Command::new(exe)
        .current_dir(cwd)
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn the built executable");
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .expect("write stdin");
    child.wait_with_output().expect("wait for the executable")
}

/// `akk` with a scripted stdin.
fn akk_stdin(dir: &Path, args: &[&str], stdin: &str) -> Output {
    use std::io::Write;
    let mut child = Command::new(env!("CARGO_BIN_EXE_akk"))
        .args(args)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn the akk binary");
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin.as_bytes())
        .expect("write stdin");
    child.wait_with_output().expect("wait for akk")
}

/// A transcript with the duration of the run normalised away: everything else
/// (prompts, printed lines, the closing line's wording) must match exactly.
fn without_the_duration(log: &str) -> String {
    log.lines()
        .filter(|line| !line.starts_with("Interpreted in ") && !line.starts_with("Compiled in "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every input shape the mission lists, in one program: a single read, reads
/// inside a loop, a read that feeds a numeric comparison, and a read consumed
/// by a function call. The source carries a sentinel so the build test can
/// prove the text itself is not embedded.
const INPUTS: &str = r#"# SOURCE_ONLY_SENTINEL_5150
name :str = input("Name: ");
name ကို ဖော်ပြပါ။

i သည် 0 ဖြစ်၏။
(i < 2) ဖြစ်နေရင်
    word :str = input("word: ");
    word ကို ဖော်ပြပါ။
    i သည် i + 1 ဖြစ်၏။
ပြီး။

n :int = input("n: ");
(n > 10) ဖြစ်ရင်
    "big" ကို ပြပါ။
မဖြစ်ရင်
    "small" ကို ပြပါ။
ပြီး။

လုပ်ငန်း shout(s)သည်
    s + "!"
ပြီး။

who :str = input("who? ");
msg သည် shout(who) ဖြစ်၏။
msg ကို ဖော်ပြပါ။
"#;

const INPUTS_STDIN: &str = "David\nfoo\nbar\n42\nZaw\n";

#[test]
fn bytecode_input_matches_the_interpreter_byte_for_byte() {
    let dir = temp_dir("bytecode-input");
    let project = demo(&dir);
    write_main(&project, INPUTS);
    write_manifest(&project, "mode = \"bytecode\"\n", "");

    let exe = assert_built(&project);
    let bytes = fs::read(&exe).unwrap();
    assert!(
        !contains(&bytes, b"SOURCE_ONLY_SENTINEL_5150"),
        "the source text must not be embedded in a bytecode build"
    );
    assert!(contains(&bytes, b"AKKBC001"));

    let built = run_exe_stdin(&exe, &project, INPUTS_STDIN);
    assert_eq!(
        built.status.code(),
        Some(0),
        "stderr: {}",
        text(&built.stderr)
    );

    let interpreted = akk_stdin(&project, &["run", "src/main.akk"], INPUTS_STDIN);
    assert_eq!(
        interpreted.status.code(),
        Some(0),
        "stderr: {}",
        text(&interpreted.stderr)
    );

    let on_vm = akk_stdin(&project, &["--vm", "src/main.akk"], INPUTS_STDIN);
    assert_eq!(
        on_vm.status.code(),
        Some(0),
        "stderr: {}",
        text(&on_vm.stderr)
    );

    let from_artifact = without_timing_line(text(&built.stdout));
    let from_interpreter = without_timing_line(text(&interpreted.stdout));
    let from_vm = without_timing_line(text(&on_vm.stdout));

    assert_eq!(from_artifact, from_interpreter, "artifact vs interpreter");
    assert_eq!(from_vm, from_interpreter, "vm vs interpreter");
    // Prompts and answers share their lines, in the program's own order.
    assert_eq!(
        from_interpreter,
        "Name: David\nword: foo\nword: bar\nn: big\nwho? Zaw!\n"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bytecode_input_exit_code_matches_on_a_missing_line() {
    let dir = temp_dir("bytecode-input-eof");
    let project = demo(&dir);
    write_main(&project, INPUTS);
    write_manifest(&project, "mode = \"bytecode\"\n", "");

    let exe = assert_built(&project);
    // Stops after the loop's second word, before the numeric read.
    let truncated = "David\nfoo\nbar\n";
    let built = run_exe_stdin(&exe, &project, truncated);
    let interpreted = akk_stdin(&project, &["run", "src/main.akk"], truncated);

    assert_eq!(built.status.code(), interpreted.status.code());
    assert_ne!(built.status.code(), Some(0));
    assert_eq!(
        errors_only(&text(&built.stderr)),
        errors_only(&text(&interpreted.stderr))
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bytecode_artifact_writes_the_same_transcript_as_the_interpreter() {
    let dir = temp_dir("bytecode-transcript");
    // Two identical projects: the same source built as bytecode and as an
    // interpreter build, so both artifacts run the same program.
    let bytecode_project = demo(&dir);
    write_main(&bytecode_project, INPUTS);
    write_manifest(&bytecode_project, "mode = \"bytecode\"\n", "");
    let bytecode_exe = assert_built(&bytecode_project);

    let interpreter_project = dir.join("demo-interpreter");
    fs::create_dir_all(interpreter_project.join("src")).unwrap();
    fs::write(interpreter_project.join("src").join("main.akk"), INPUTS).unwrap();
    fs::write(
        interpreter_project.join("akk.toml"),
        "[package]\nname = \"demo-interpreter\"\nversion = \"0.1.0\"\nentry = \"src/main.akk\"\n\n[build]\nout_dir = \"build\"\nmode = \"interpreter\"\n\n[run]\ntranscript = true\nwarnings = true\ntiming = true\n",
    )
    .unwrap();
    let out = akk(&interpreter_project, &["build"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", text(&out.stderr));
    let interpreter_exe = interpreter_project
        .join("build")
        .join(format!("demo-interpreter{}", std::env::consts::EXE_SUFFIX));
    assert!(interpreter_exe.is_file());

    let built = run_exe_stdin(&bytecode_exe, &bytecode_project, INPUTS_STDIN);
    assert_eq!(
        built.status.code(),
        Some(0),
        "stderr: {}",
        text(&built.stderr)
    );
    let reference = run_exe_stdin(&interpreter_exe, &interpreter_project, INPUTS_STDIN);
    assert_eq!(
        reference.status.code(),
        Some(0),
        "stderr: {}",
        text(&reference.stderr)
    );

    // Each artifact writes its transcript beside itself, named after its stem.
    let bytecode_log = bytecode_exe.with_extension("akop");
    let interpreter_log = interpreter_exe.with_extension("akop");
    assert!(bytecode_log.is_file(), "missing {}", bytecode_log.display());
    assert!(
        interpreter_log.is_file(),
        "missing {}",
        interpreter_log.display()
    );

    let bytecode_text = text(&fs::read(&bytecode_log).unwrap());
    let interpreter_text = text(&fs::read(&interpreter_log).unwrap());
    assert_eq!(
        without_the_duration(&bytecode_text),
        without_the_duration(&interpreter_text),
        "the bytecode transcript must match the interpreter's"
    );
    // The transcript holds the prompts and answers, in order.
    assert!(bytecode_text.starts_with("Name: David\nword: foo\nword: bar\n"));
    assert!(
        !bytecode_text.contains("Interpreted in ") && !bytecode_text.contains("Compiled in "),
        "a built file prints no timer: {bytecode_text}"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bytecode_transcript_honours_the_transcript_flag() {
    let dir = temp_dir("bytecode-transcript-flags");
    let project = demo(&dir);
    write_main(
        &project,
        "name :str = input(\"Name: \");\nname ကို ဖော်ပြပါ။\n",
    );
    write_manifest(&project, "mode = \"bytecode\"\n", "");
    let manifest = fs::read_to_string(project.join("akk.toml")).unwrap();
    fs::write(
        project.join("akk.toml"),
        manifest.replace("transcript = true", "transcript = false"),
    )
    .unwrap();

    let exe = assert_built(&project);
    let out = run_exe_stdin(&exe, &project, "David\n");
    assert_eq!(out.status.code(), Some(0), "stderr: {}", text(&out.stderr));
    assert!(!exe.with_extension("akop").exists());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn build_bytecode_still_refuses_the_app_gui_library() {
    let dir = temp_dir("bytecode-app");
    let project = demo(&dir);
    write_manifest(&project, "mode = \"bytecode\"\n", "");
    write_main(
        &project,
        "နည်းပညာများ App ကို သုံးမည်။\nw သည် App.screen(400, 300) ဖြစ်၏။\n\"hi\" ကို ပြပါ။\n",
    );

    let out = build(&project);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("E170"),
        "stderr: {}",
        text(&out.stderr)
    );
    assert!(!built_exe(&project).exists());
    assert!(!project.join("build").join("demo.akop").exists());

    let _ = fs::remove_dir_all(&dir);
}
/// Eng-spelled declarations, a template string and loop control, built as
/// bytecode: the artifact, the interpreter and the VM must agree.
const ENG_DECLS: &str = r#"# SOURCE_ONLY_SENTINEL_8080
name :str = input("Name: ");
print("Hello, {name}!");

pin LIMIT :int = 3;
total :int = 0;
i သည် 0 ဖြစ်၏။
while (i < LIMIT) {
    i = i + 1;
    if (i == 2) {
        continue;
    }
    total += i;
}
print(total);
print(LIMIT);
"#;

#[test]
fn bytecode_builds_eng_declaration_programs() {
    let dir = temp_dir("bytecode-eng-decls");
    let project = demo(&dir);
    write_main(&project, ENG_DECLS);
    write_manifest(&project, "mode = \"bytecode\"\n", "");

    let exe = assert_built(&project);
    let bytes = fs::read(&exe).unwrap();
    assert!(
        !contains(&bytes, b"SOURCE_ONLY_SENTINEL_8080"),
        "the source text must not be embedded in a bytecode build"
    );
    assert!(contains(&bytes, b"AKKBC001"));

    let built = run_exe_stdin(&exe, &project, "Dara\n");
    assert_eq!(
        built.status.code(),
        Some(0),
        "stderr: {}",
        text(&built.stderr)
    );
    let interpreted = akk_stdin(&project, &["run", "src/main.akk"], "Dara\n");
    assert_eq!(
        interpreted.status.code(),
        Some(0),
        "stderr: {}",
        text(&interpreted.stderr)
    );
    let on_vm = akk_stdin(&project, &["--vm", "src/main.akk"], "Dara\n");
    assert_eq!(
        on_vm.status.code(),
        Some(0),
        "stderr: {}",
        text(&on_vm.stderr)
    );

    let from_artifact = without_timing_line(text(&built.stdout));
    let from_interpreter = without_timing_line(text(&interpreted.stdout));
    assert_eq!(from_artifact, from_interpreter, "artifact vs interpreter");
    assert_eq!(
        without_timing_line(text(&on_vm.stdout)),
        from_interpreter,
        "vm vs interpreter"
    );
    // 1 + 3 = 4 (the `continue` skips the 2), then the pinned 3.
    assert_eq!(from_interpreter, "Name: Hello, Dara!\n4\n3\n");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bytecode_pin_and_type_errors_match_the_interpreter() {
    let dir = temp_dir("bytecode-eng-errors");
    let project = demo(&dir);
    let program = "pin N :int = 1;\nprint(N);\nN = 2;\nprint(\"unreachable\");\n";
    write_main(&project, program);
    write_manifest(&project, "mode = \"bytecode\"\n", "");

    let exe = assert_built(&project);
    let built = run_exe(&exe, &project, &[]);
    let interpreted = akk(&project, &["run", "src/main.akk"]);

    assert_eq!(built.status.code(), Some(1));
    assert_eq!(built.status.code(), interpreted.status.code());
    assert_eq!(
        without_timing_line(text(&built.stdout)),
        without_timing_line(text(&interpreted.stdout))
    );
    let built_err = errors_only(&text(&built.stderr));
    let interpreted_err = errors_only(&text(&interpreted.stderr));
    assert_eq!(built_err, interpreted_err);
    assert!(built_err.contains("E104"), "{built_err}");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bytecode_try_catch_finally_matches_the_interpreter() {
    let dir = temp_dir("bytecode-try-catch");
    let project = demo(&dir);
    let program = "print(\"start\");\n\n# a caught error: the catch body and then finally run, in that order\nစမ်းရန်\n    n :int = \"no\";\nE103 ကို ဖမ်းပါ။\n    print(\"caught E103\");\nနောက်ဆုံးတွင်\n    print(\"fin\");\nပြီး။\n\n# a nested try whose inner filter does not match: the outer one catches\nစမ်းရန်\n    စမ်းရန်\n        m :int = \"no\";\n    E999 ကို ဖမ်းပါ။\n        print(\"wrong\");\n    နောက်ဆုံးတွင်\n        print(\"inner fin\");\n    ပြီး။\nE103 ကို ဖမ်းပါ။\n    print(\"outer caught\");\nပြီး။\n\n# an error raised inside a called function, caught by the caller\nfn boom() {\n    x :int = \"no\";\n}\nစမ်းရန်\n    boom();\nE ကို ဖမ်းပါ။\n    print(\"caught a call\");\nပြီး။\nprint(\"end\");\n";
    write_main(&project, program);
    write_manifest(&project, "mode = \"bytecode\"\n", "");

    let exe = assert_built(&project);
    let built = run_exe(&exe, &project, &[]);
    assert_eq!(
        built.status.code(),
        Some(0),
        "stderr: {}",
        text(&built.stderr)
    );
    let interpreted = akk(&project, &["run", "src/main.akk"]);
    let on_vm = akk(&project, &["--vm", "src/main.akk"]);
    assert_eq!(interpreted.status.code(), Some(0));
    assert_eq!(
        on_vm.status.code(),
        Some(0),
        "stderr: {}",
        text(&on_vm.stderr)
    );

    let from_interpreter = without_timing_line(text(&interpreted.stdout));
    assert_eq!(
        without_timing_line(text(&built.stdout)),
        from_interpreter,
        "artifact vs interpreter"
    );
    assert_eq!(
        without_timing_line(text(&on_vm.stdout)),
        from_interpreter,
        "vm vs interpreter"
    );
    assert_eq!(
        from_interpreter,
        "start\ncaught E103\nfin\ninner fin\nouter caught\ncaught a call\nend\n"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bytecode_uncaught_error_inside_a_try_matches_the_interpreter() {
    let dir = temp_dir("bytecode-try-uncaught");
    let project = demo(&dir);
    let program = "print(\"before\");\nစမ်းရန်\n    n :int = \"no\";\nE999 ကို ဖမ်းပါ။\n    print(\"wrong\");\nနောက်ဆုံးတွင်\n    print(\"fin\");\nပြီး။\nprint(\"unreachable\");\n";
    write_main(&project, program);
    write_manifest(&project, "mode = \"bytecode\"\n", "");

    let exe = assert_built(&project);
    let built = run_exe(&exe, &project, &[]);
    let interpreted = akk(&project, &["run", "src/main.akk"]);
    let on_vm = akk(&project, &["--vm", "src/main.akk"]);

    assert_eq!(built.status.code(), Some(1));
    assert_eq!(on_vm.status.code(), Some(1));
    assert_eq!(interpreted.status.code(), Some(1));
    let from_interpreter = without_timing_line(text(&interpreted.stdout));
    assert_eq!(without_timing_line(text(&built.stdout)), from_interpreter);
    assert_eq!(without_timing_line(text(&on_vm.stdout)), from_interpreter);
    assert_eq!(from_interpreter, "before\nfin\n");
    let built_err = errors_only(&text(&built.stderr));
    assert_eq!(built_err, errors_only(&text(&interpreted.stderr)));
    assert!(built_err.contains("E103"), "{built_err}");
    // A bytecode container deliberately carries instructions, not source, so
    // the built artifact prints the message alone. `--vm` reads the source
    // and adds the report under the same message.
    let on_vm_err = errors_only(&text(&on_vm.stderr));
    assert!(
        on_vm_err.starts_with(&built_err),
        "vm message differs: {on_vm_err}"
    );
    assert!(
        on_vm_err.contains("\n\nline 3|     n :int = \"no\";\n"),
        "{on_vm_err}"
    );
    assert!(on_vm_err.ends_with("အမျိုးအစား မကိုက်ညီ"), "{on_vm_err}");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bytecode_loop_values_match_the_interpreter() {
    let dir = temp_dir("bytecode-loop-value");
    let project = demo(&dir);
    let program = "print(\"start\");\n\n# loop as a value: a typed declaration, accumulating into its own name\nx :int = loop {\n    x += 1;\n    if (x == 3) {\n        break x * 10;\n    }\n};\nprint(x);\n\n# the assignment spelling, with a break inside try/finally\nn :int = loop {\n    စမ်းရန်\n        print(\"body\");\n        break 7;\n    နောက်ဆုံးတွင်\n        print(\"fin\");\n    ပြီး။\n};\nprint(n);\n\n# nested: the inner loop value feeds an outer while loop\ntotal :int = 0;\ni :int = 0;\nwhile (i < 3) {\n    i += 1;\n    inner :int = loop {\n        if (i == 2) {\n            break 100;\n        }\n        break i;\n    };\n    total += inner;\n}\nprint(total);\n\n# a plain loop: break without a value, and a value that is not used\nloop {\n    i += 1;\n    if (i == 5) {\n        break 99;\n    }\n}\nprint(i);\nprint(\"end\");\n";
    write_main(&project, program);
    write_manifest(&project, "mode = \"bytecode\"\n", "");

    let exe = assert_built(&project);
    let built = run_exe(&exe, &project, &[]);
    assert_eq!(
        built.status.code(),
        Some(0),
        "stderr: {}",
        text(&built.stderr)
    );
    let interpreted = akk(&project, &["run", "src/main.akk"]);
    let on_vm = akk(&project, &["--vm", "src/main.akk"]);
    assert_eq!(interpreted.status.code(), Some(0));
    assert_eq!(
        on_vm.status.code(),
        Some(0),
        "stderr: {}",
        text(&on_vm.stderr)
    );

    let from_interpreter = without_timing_line(text(&interpreted.stdout));
    assert_eq!(
        without_timing_line(text(&built.stdout)),
        from_interpreter,
        "artifact vs interpreter"
    );
    assert_eq!(
        without_timing_line(text(&on_vm.stdout)),
        from_interpreter,
        "vm vs interpreter"
    );
    assert_eq!(from_interpreter, "start\n30\nbody\n7\n104\n5\nend\n");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bytecode_builtin_program_matches_the_interpreter() {
    let dir = temp_dir("bytecode-builtins");
    let project = demo(&dir);
    let program = "print(len(\"abc\"));\nprint(abs(-5));\nprint(max(3, 7));\nprint(sqrt(6.25));\nprint(floor(2.9));\nprint(ceil(8.1));\nprint(find(\"abcd\", \"d\"));\nprint(min(2.5, 3.5));\nprint(sum([1, 1, 2]));\nprint(upper(\"eng\"));\nprint(lower(\"ENGLISH\"));\nprint(trim(\"  Akkhara  \"));\nprint(contains(\"abc\", \"b\"));\nprint(starts_with(\"abc\", \"ab\"));\nprint(len(split(\"a,b,c,d\", \",\")));\nစမ်းရန်\n    sqrt(-1);\nE111 ကို ဖမ်းပါ။\n    print(\"sqrt ok\");\nပြီး။\n";
    write_main(&project, program);
    write_manifest(&project, "mode = \"bytecode\"\n", "");

    let exe = assert_built(&project);
    let built = run_exe(&exe, &project, &[]);
    assert_eq!(
        built.status.code(),
        Some(0),
        "stderr: {}",
        text(&built.stderr)
    );
    let interpreted = akk(&project, &["run", "src/main.akk"]);
    let on_vm = akk(&project, &["--vm", "src/main.akk"]);
    assert_eq!(interpreted.status.code(), Some(0));
    assert_eq!(
        on_vm.status.code(),
        Some(0),
        "stderr: {}",
        text(&on_vm.stderr)
    );

    let from_interpreter = without_timing_line(text(&interpreted.stdout));
    assert_eq!(
        without_timing_line(text(&built.stdout)),
        from_interpreter,
        "artifact vs interpreter"
    );
    assert_eq!(
        without_timing_line(text(&on_vm.stdout)),
        from_interpreter,
        "vm vs interpreter"
    );
    assert_eq!(
        from_interpreter,
        "3\n5\n7\n2.5\n2\n9\n3\n2.5\n4\nENG\nenglish\nAkkhara\nTrue\nTrue\n4\nsqrt ok\n"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bytecode_library_program_matches_the_interpreter() {
    let dir = temp_dir("bytecode-libraries");
    let project = demo(&dir);
    let program = "use ကျပန်း;\nprint(ကျပန်း.ကိန်း(5, 6));\nprint(ကျပန်း.တန်ဖိုး([\"A\"]));\n\n# a library error is catchable, and its message is the interpreter's\nစမ်းရန်\n    nosuchlib.get(\"x\");\nE085 ကို ဖမ်းပါ။\n    print(\"E085 ok\");\nပြီး။\n\n# a second library, imported later in the program\nuse အချိန်;\nprint(အချိန်.ယခု() > 0);\n";
    write_main(&project, program);
    write_manifest(&project, "mode = \"bytecode\"\n", "");

    let exe = assert_built(&project);
    let built = run_exe(&exe, &project, &[]);
    assert_eq!(
        built.status.code(),
        Some(0),
        "stderr: {}",
        text(&built.stderr)
    );
    let interpreted = akk(&project, &["run", "src/main.akk"]);
    let on_vm = akk(&project, &["--vm", "src/main.akk"]);
    assert_eq!(interpreted.status.code(), Some(0));
    assert_eq!(
        on_vm.status.code(),
        Some(0),
        "stderr: {}",
        text(&on_vm.stderr)
    );

    let from_interpreter = without_timing_line(text(&interpreted.stdout));
    assert_eq!(
        without_timing_line(text(&built.stdout)),
        from_interpreter,
        "artifact vs interpreter"
    );
    assert_eq!(
        without_timing_line(text(&on_vm.stdout)),
        from_interpreter,
        "vm vs interpreter"
    );
    assert_eq!(from_interpreter, "5\nA\nE085 ok\nTrue\n");

    let _ = fs::remove_dir_all(&dir);
}
