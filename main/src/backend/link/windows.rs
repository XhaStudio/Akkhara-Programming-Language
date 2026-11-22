//! Writing a Windows executable: a PE image needs no permission change, the
//! `.exe` suffix is what makes it runnable.

use std::path::Path;

pub(crate) fn write_file(out: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(out, bytes)
        .map_err(|error| format!("E168 \"{}\" ကို ရေး၍မရပါ - {}", out.display(), error))
}
