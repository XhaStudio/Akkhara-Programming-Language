// Akkhara file library ("file" / "ဖိုင်")
//
// Usage (from an .akk program): import it, then call through the library name,
// in either spelling:
//
//     နည်းပညာများ file ကို အသုံးပြုပါ။
//     text အတွက် file ၏ read("notes.txt") ကို လုပ်ပါ။
//     text ကို ဖော်ပြပါ။
//
//     use file;                          # the eng spelling of the import
//     file.write("notes.txt", "hello");
//
// This module owns the disk I/O only: paths in, bytes out, and a plain error
// string on failure. Which path a program is allowed to touch, what a missing
// file means, and how an error is worded in Myanmar are the interpreter's
// business (see `eval_file_call` in src/interpreter.rs), so this side stays
// testable as plain Rust and the language side stays in one place.
//
// Every function works on UTF-8 text: a file that is not valid UTF-8 is a
// read error rather than silently mangled. `write` truncates and does not
// create missing folders; `append` creates the file if it is not there yet.

use std::fs;
use std::io::Write;
use std::path::Path;

/// Read a whole file as UTF-8 text.
pub fn read(path: &str) -> Result<String, String> {
    match fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|_| not_text(path)),
        Err(e) => Err(e.to_string()),
    }
}

/// Create or truncate `path` and write `text` to it.
pub fn write(path: &str, text: &str) -> Result<(), String> {
    fs::write(path, text.as_bytes()).map_err(|e| e.to_string())
}

/// Append `text` to `path`, creating the file when it does not exist yet.
pub fn append(path: &str, text: &str) -> Result<(), String> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(text.as_bytes()).map_err(|e| e.to_string())
}

/// Whether anything (file or folder) exists at `path`.
pub fn exists(path: &str) -> bool {
    Path::new(path).exists()
}

/// The entry names inside `folder`, sorted. The names are returned as they are
/// on disk (no folder prefix), so `list("docs")` might give `["a.txt", "b.txt"]`.
pub fn list(folder: &str) -> Result<Vec<String>, String> {
    let entries = fs::read_dir(folder).map_err(|e| e.to_string())?;
    let mut names: Vec<String> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    Ok(names)
}

/// Delete a file. A missing path is an error (its message names the path), so
/// a program can tell "deleted" from "was never there".
pub fn delete(path: &str) -> Result<(), String> {
    fs::remove_file(path).map_err(|e| format!("{} ({})", e, path))
}

/// A file's lines, without their line endings. Both `\n` and `\r\n` are
/// recognized, so a file written on Windows reads the same everywhere.
pub fn lines(path: &str) -> Result<Vec<String>, String> {
    Ok(read(path)?.lines().map(String::from).collect())
}

/// A file's size in bytes. Text in Myanmar script is UTF-8, so this counts
/// bytes rather than characters (`len` on the text counts characters).
pub fn size(path: &str) -> Result<u64, String> {
    fs::metadata(path)
        .map(|m| m.len())
        .map_err(|e| e.to_string())
}

fn not_text(path: &str) -> String {
    format!("{} is not valid UTF-8 text", path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A fresh folder for one test, under the system temp folder.
    fn temp_dir(tag: &str) -> std::path::PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "akk-file-{}-{}-{}",
            tag,
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp folder");
        dir
    }

    #[test]
    fn write_read_append_and_lines_round_trip() {
        let dir = temp_dir("round-trip");
        let path = dir.join("notes.txt");
        let path = path.to_str().expect("utf-8 path");

        assert!(!exists(path));
        write(path, "first\nsecond").expect("write");
        assert!(exists(path));
        assert_eq!(read(path).expect("read"), "first\nsecond");
        assert_eq!(size(path).expect("size"), 12);

        append(path, "\nthird").expect("append");
        assert_eq!(
            lines(path).expect("lines"),
            vec![
                "first".to_string(),
                "second".to_string(),
                "third".to_string()
            ]
        );

        // Writing again truncates: the old contents do not survive.
        write(path, "only").expect("rewrite");
        assert_eq!(read(path).expect("read"), "only");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn list_is_sorted_and_delete_removes_a_file() {
        let dir = temp_dir("list");
        write(dir.join("b.txt").to_str().unwrap(), "b").expect("write b");
        write(dir.join("a.txt").to_str().unwrap(), "a").expect("write a");

        let names = list(dir.to_str().unwrap()).expect("list");
        assert_eq!(names, vec!["a.txt".to_string(), "b.txt".to_string()]);

        let a = dir.join("a.txt");
        delete(a.to_str().unwrap()).expect("delete");
        assert!(!exists(a.to_str().unwrap()));
        // Deleting it twice reports an error instead of silently succeeding.
        assert!(delete(a.to_str().unwrap()).is_err());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_file_is_an_error_not_empty_text() {
        let dir = temp_dir("missing");
        let missing = dir.join("nope.txt");
        assert!(!exists(missing.to_str().unwrap()));
        assert!(read(missing.to_str().unwrap()).is_err());
        assert!(size(missing.to_str().unwrap()).is_err());
        fs::remove_dir_all(&dir).ok();
    }
}
