//! Akkhara (အက္ခရာ): a Myanmar-language programming language.
//!
//! `main` does one thing -- call [`cli::run`]. Everything else lives in its
//! own module: the front end (`frontend`), the tree-walking interpreter
//! (`interp`), the bytecode engine (`vm`), the static warnings (`sema`), the
//! compiled-in libraries (`stdlib`), and the project files `akk build` and
//! `akk.toml` are made of (`project`).
//!
//! `types`, `scope`, `ir` and `opt` are the compiler middle end: the type
//! names, the name resolver, the IR that `akk ir` prints, and the constant
//! folding and dead-code elimination `akk ir --opt` runs over it. `backend`
//! turns MIR into machine code and an executable file for a narrow slice of
//! the language, and `runtime` is the `print`/`exit`/`panic` code linked into
//! what it writes.

mod backend;
mod cli;
mod diagnostics;
mod frontend;
mod interp;
mod ir;
mod opt;
mod project;
mod runtime;
mod scope;
mod sema;
mod stdlib;
mod text;
mod types;
mod vm;

fn main() {
    std::process::exit(cli::run());
}
