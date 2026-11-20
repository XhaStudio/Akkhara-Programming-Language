//! Executable image writers, one per platform container.
//!
//! `elf` and `pe` produce a complete runnable file; `macho` is a note until
//! there is an aarch64/macOS backend to write for.

pub(crate) mod elf;
pub(crate) mod macho;
pub(crate) mod pe;
