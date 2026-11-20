//! The code linked into a native executable: `write`/`exit` shims for the
//! platform, `print` for int/bool/string, and the panic used when a modulo is
//! taken by zero.
//!
//! These are machine-code routines, emitted once per executable by [`emit`]
//! and reached by `call` from the generated `main`. Nothing here depends on
//! libc: on Linux the shims are syscalls, on Windows they are calls through a
//! tiny import table the image writer builds.

pub(crate) mod float;
pub(crate) mod panic;
pub(crate) mod print;

use crate::backend::x86_64::{Code, Data, Import};

/// The platform a native executable is being written for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    LinuxX86_64,
    WindowsX86_64,
}

impl Target {
    /// The target this host can produce and run, if it is one of the two.
    pub(crate) fn host() -> Option<Target> {
        if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            Some(Target::LinuxX86_64)
        } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            Some(Target::WindowsX86_64)
        } else {
            None
        }
    }
}

/// Emit every routine the generated code calls: the platform shims, the three
/// printers and the modulo-by-zero panic.
pub(crate) fn emit(code: &mut Code, data: &mut Data, target: Target, imports: &mut Vec<Import>) {
    // The import slots are reserved before any string, so they stay one
    // contiguous `FirstThunk` array for the Windows import descriptor.
    let platform = print::declare_imports(data, imports, target);
    print::emit_write_and_exit(code, &platform);
    print::emit_int(code, data);
    float::emit(code);
    print::emit_bool(code, data);
    print::emit_str(code, data);
    panic::emit_zero(code, data);
    panic::emit_div(code, data);
}
