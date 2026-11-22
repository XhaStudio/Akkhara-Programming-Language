//! `akk new <name>`: a manifest, an ignore file and a first program.

use std::fs;
use std::path::{Path, PathBuf};

/// The folder ignore file `akk new` writes: the default `[build] out_dir` and
/// the `.akop` transcripts `save_terminal_log` leaves next to a program.
pub(crate) const NEW_GITIGNORE: &str = "/build/\n*.akop\n";

/// A project name `akk new` accepts: one folder component that is safe on
/// every platform we ship. The rule is a blacklist plus a few structural
/// checks -- deliberately *not* an alphanumeric whitelist, because Myanmar
/// letters and combining marks (e.g. U+1039) must stay usable. The limit is
/// 255 bytes: one Myanmar letter is 3 bytes, so a smaller cap would refuse
/// ordinary names.
pub(crate) fn valid_project_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 255 {
        return false;
    }
    if name == "." || name == ".." || name.starts_with('-') {
        return false;
    }
    // Reserved device names, with or without an extension (`CON`, `con.txt`).
    let stem = name.split('.').next().unwrap_or(name);
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        return false;
    }
    name.chars().all(|c| {
        !c.is_whitespace()
            && !c.is_control()
            && !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
    })
}

/// Create the folder tree and write the three files. Split out so `cmd_new`
/// can delete the folder when any step fails: a half-written project would
/// make the next `akk new <name>` fail with E166.
pub(crate) fn create_project(
    root: &Path,
    manifest_text: &str,
    program_text: &str,
) -> std::io::Result<()> {
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir)?;
    fs::write(root.join("akk.toml"), manifest_text)?;
    fs::write(root.join(".gitignore"), NEW_GITIGNORE)?;
    fs::write(src_dir.join("main.akk"), program_text)?;
    Ok(())
}

/// `akk new <name>` -- create `<name>/` with a manifest, an ignore file and a
/// first program, then print how to enter it. Nothing outside the new folder
/// is touched, and no `git` command runs.
///
/// `args.get(2)` hands an explicitly empty argument through as `Some("")`; an
/// empty name cannot become a folder, so it is treated like a missing name and
/// prints the usage line.
pub(crate) fn cmd_new(name: Option<&str>) -> i32 {
    let Some(name) = name.filter(|n| !n.is_empty()) else {
        eprintln!("Usage: akk new <name>");
        return 1;
    };

    if !valid_project_name(name) {
        eprintln!(
            "E167 akk new: \"{}\" ကို ပရောဂျက် အမည်အဖြစ် မသုံးနိုင်ပါ။ အမည်တွင် ကွက်လပ်၊ / \\ : * ? \" < > | နှင့် control character မပါရပါ။",
            name
        );
        return 1;
    }

    let root = PathBuf::from(name);
    // `symlink_metadata` also succeeds for a dangling symlink, which `exists()`
    // reports as absent: creating into it would follow the link, and the
    // cleanup below would then delete a path this command never created.
    if root.symlink_metadata().is_ok() {
        eprintln!("E166 akk new: \"{}\" သည် ရှိပြီးဖြစ်ပါသည်။ အခြားအမည်ပေးပါ။", name);
        return 1;
    }

    // Every key below is read by `manifest::load` (`PACKAGE_KEYS`,
    // `BUILD_KEYS`, `RUN_KEYS`), so the generated file raises no W008.
    let manifest_text = format!(
        "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nentry = \"src/main.akk\"\n\n[build]\nout_dir = \"build\"\n\n[run]\ntranscript = true\nwarnings = true\ntiming = true\n",
        name
    );
    let program_text = format!("# {}\n\"မင်္ဂလာပါ!\" ကို ဖော်ပြပါ။\n", name);

    if let Err(e) = create_project(&root, &manifest_text, &program_text) {
        let _ = fs::remove_dir_all(&root);
        eprintln!("akk new: ဖိုဒါ ဖန်တီး၍မရပါ - {}", e);
        return 1;
    }

    println!(
        "akk new: \"{}\" ဖိုဒါကို ဖန်တီးပြီးပါပြီ။ \"cd {}\" ဖြင့် ဝင်ပါ။",
        name, name
    );
    0
}

#[cfg(test)]
mod new_tests {
    use super::valid_project_name;

    #[test]
    fn accepts_plain_and_myanmar_names() {
        for name in ["demo", "my-app", "app_2", "မြန်မာ", "အက္ခရာ"] {
            assert!(valid_project_name(name), "{name} should be accepted");
        }
    }

    #[test]
    fn name_length_limit_is_255_bytes() {
        assert!(valid_project_name(&"a".repeat(255)));
        assert!(!valid_project_name(&"a".repeat(256)));
        // 85 Myanmar letters are 255 bytes; 86 are 258.
        assert!(valid_project_name(&"က".repeat(85)));
        assert!(!valid_project_name(&"က".repeat(86)));
    }

    #[test]
    fn rejects_unsafe_names() {
        for name in [
            "", ".", "..", "-x", "a/b", "a\\b", "my app", "a:b", "CON", "con.txt", "a*b", "a\tb",
        ] {
            assert!(!valid_project_name(name), "{name} should be rejected");
        }
    }
}
