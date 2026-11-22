//! Putting the finished image on disk.
//!
//! There is no external linker in this pipeline -- the object writer already
//! produced a complete file -- so "linking" is writing the bytes and, on
//! Linux, marking the file executable.

pub(crate) mod linux;
pub(crate) mod windows;

use std::path::Path;

/// Write `bytes` to `out` for the host platform.
pub(crate) fn write(out: &Path, bytes: &[u8]) -> Result<(), String> {
    if cfg!(windows) {
        windows::write_file(out, bytes)
    } else {
        linux::write_file(out, bytes)
    }
}
