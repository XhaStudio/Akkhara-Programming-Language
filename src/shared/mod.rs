//! Types and text both engines share.
//!
//! The front end, the middle end and the native backend must not reach into
//! `interp`: a symbol belongs to the program, not to one way of running it,
//! and the wording of an error is part of the compiler's contract whichever
//! engine reports it. The test at the bottom of this module enforces that.

pub(crate) mod errors;
pub(crate) mod sym;

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    /// Every `.rs` file under `dir`, recursively.
    fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                rust_files(&path, found);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                found.push(path);
            }
        }
    }

    /// `Sym` and the shared error text live here so that the front end, the
    /// middle end and the backend do not have to depend on the interpreter.
    /// A path such as `crate::interp::Sym` in any of them would make the
    /// interpreter a build-time dependency of code that must run without it.
    #[test]
    fn shared_frontend_middle_end_and_backend_do_not_reach_into_the_interpreter() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        for module in ["shared", "frontend", "ir", "backend"] {
            let mut files = Vec::new();
            rust_files(&root.join(module), &mut files);
            for file in files {
                // This test names the forbidden path in its own body.
                if file == root.join("shared").join("mod.rs") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&file) else {
                    continue;
                };
                for (number, line) in text.lines().enumerate() {
                    if line.trim_start().starts_with("//") {
                        continue;
                    }
                    if line.contains("crate::interp") {
                        offenders.push(format!(
                            "{}:{}: {}",
                            file.display(),
                            number + 1,
                            line.trim()
                        ));
                    }
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "these modules must not depend on the interpreter:\n{}",
            offenders.join("\n")
        );
    }
}
