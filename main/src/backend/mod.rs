//! The native backend: MIR in, an executable file out.
//!
//! One target is implemented -- x86-64, for Linux (`elf`) and Windows (`pe`) --
//! and only a slice of the language: functions over `int`, `float`, `bool`
//! and string literals, with arithmetic, comparisons (strings included),
//! `print`, calls, `return` and the numeric builtins
//! `abs`/`min`/`max`/`floor`/`ceil`/`round`.
//! Everything else is refused with `E177` by
//! [`x86_64::analyze`] before a single byte is emitted, so an unsupported
//! program cannot become silently different code.
//!
//! No assembler or linker is used. The instructions are encoded by
//! `x86_64::encode`, `object` writes the file format, `link` puts the bytes on
//! disk and makes them executable, and `runtime` emits the `write`/`exit`/
//! `print` routines that are linked in.

pub(crate) mod link;
pub(crate) mod object;
pub(crate) mod x86_64;

use std::path::Path;

use crate::ir::mir::MirProgram;
use crate::runtime::Target;

pub(crate) use x86_64::Image;

/// What every target must provide.
pub(crate) trait Backend {
    /// Machine code and data for `program`.
    fn image(&self, program: &MirProgram) -> Result<Image, String>;

    /// The image wrapped in this platform's executable file format.
    fn executable(&self, image: &mut Image) -> Result<Vec<u8>, String>;
}

struct Native {
    target: Target,
}

impl Backend for Native {
    fn image(&self, program: &MirProgram) -> Result<Image, String> {
        x86_64::compile(program, self.target)
    }

    fn executable(&self, image: &mut Image) -> Result<Vec<u8>, String> {
        match self.target {
            Target::LinuxX86_64 => Ok(object::elf::write(image)),
            Target::WindowsX86_64 => object::pe::write(image),
        }
    }
}

/// The backend for the machine this compiler is running on, if one exists.
pub(crate) fn host() -> Result<Box<dyn Backend>, String> {
    match Target::host() {
        Some(target) => Ok(Box::new(Native { target })),
        None => Err(x86_64::unsupported(&format!(
            "this platform ({} on {})",
            std::env::consts::ARCH,
            std::env::consts::OS
        ))),
    }
}

/// `akk build <file>`: parse a program, lower it to MIR, optionally optimise
/// it, generate machine code, and write the executable to `out`.
pub(crate) fn build_source(source: &str, out: &Path, optimize: bool) -> Result<(), String> {
    let tokens = crate::frontend::lexer::lex_source(source)?;
    let stmts = crate::frontend::parser::parse_source(&tokens, source)?;
    let hir = crate::ir::lower::hir(&stmts);
    let mut program = match crate::ir::lower::mir(&hir) {
        Ok(program) => program,
        // The middle end will not model it (`E176`), so the native backend
        // cannot emit it either. `akk build` says that with the backend's own
        // code and quotes the reason, so the construct is still named.
        Err(reason) if reason.starts_with("E176") => {
            return Err(crate::shared::errors::native_refusal(&reason))
        }
        Err(reason) => return Err(reason),
    };
    // Decide the slice before anything is optimised: folding is not allowed to
    // hide a construct the backend cannot emit (constant folding can turn a
    // refused expression into a value, but it is still outside the slice and
    // must be refused the same way with `--no-opt`).
    x86_64::analyze(&program)?;
    if optimize {
        crate::opt::optimize(&mut program);
    }
    let backend = host()?;
    let mut image = backend.image(&program)?;
    let bytes = backend.executable(&mut image)?;
    link::write(out, &bytes)
}
