# Architecture

Akkhara (`akk`) is a Myanmar-language programming language implemented in Rust.
This document describes the source tree as it exists today and, just as
importantly, which parts of it run.

## Entry point

`src/main.rs` declares the modules and does nothing else:

```rust
fn main() {
    std::process::exit(cli::run());
}
```

`cli::run()` (`src/cli/mod.rs`) creates the `libraries/` folder beside the
executable, removes the global flags (`--no-warn`, `--vm`) from the argument
list, and dispatches to one command:

| Command | Module |
| --- | --- |
| `akk <file>`, `akk run` | `cli::run` |
| `akk --vm <file>` | `cli::run` → `vm` |
| `akk build` | `cli::build` → `project::bundle` |
| `akk build <file> [--no-opt]` | `cli::build` → `backend` (native executable, below) |
| `akk new` | `cli::new` |
| `akk install` | `cli::install` |
| `akk update` | `cli::update` |
| `akk check` | `cli::check` → `sema::check` |
| `akk report` | `cli::run` |
| `akk ir [--opt] <file>` | `cli::run` → `ir::dump_file` (symbol table, HIR and MIR; `--opt` adds the optimised MIR) |

## The tree

```text
src/
├─ main.rs                  entry point: declare modules, call cli::run()
├─ cli/                     argument parsing and the commands above
├─ frontend/
│  ├─ lexer/                text -> tokens (Myanmar and English keywords)
│  ├─ parser/               tokens -> syntax tree
│  └─ ast/                  the syntax-tree node types
├─ types/                   the Type enum and per-type notes
├─ scope/                   name resolution: bindings, frames, captures
├─ sema/                    static warnings that run before a program executes
├─ ir/                      HIR, MIR and their lowering (`akk ir`)
├─ opt/                     constant folding and dead-code elimination
├─ backend/                 MIR -> machine code, object writers and linkers
├─ runtime/                 write/print/panic code linked into an executable
├─ interp/                  the tree-walking interpreter (the reference)
├─ vm/                      bytecode instructions, compiler and virtual machine
├─ stdlib/                  the libraries compiled into the binary
├─ project/                 akk.toml, library resolution, akk build bundling
├─ diagnostics/             the code registry, planned span and renderer
└─ text/                    grapheme-cluster helpers
tests/                      CLI, project, bundle and native integration tests
docs/                       this document
```

## What actually runs

Everything in this section is compiled into the `akk` binary and runs when the
matching feature is used.

| Module | Role |
| --- | --- |
| `frontend::lexer` | Turns source text into tokens. Keyword constants live in `keywords_my.rs` / `keywords_en.rs`; `token.rs` holds `Token`; `error.rs` holds `E00x` lexer messages. |
| `frontend::parser` | Recursive-descent parser. Split by construct (`expr`, `stmt`, `cond`, `func`, `class`), with `error.rs` for parse errors and `mod.rs` for the shared state and keyword predicates. |
| `frontend::ast` | `Expr` / `Stmt` node types the parser builds and the interpreter walks. |
| `types` | The `Type` enum, `named()` (both spellings), `name_mm()` / `name_en()`, and a test that every type round-trips through its name. |
| `scope` | `symbol.rs` (the symbol table and `VarId`), `resolver.rs` (frames, `declare` / `resolve` / `declare_or_resolve`), `capture.rs` (a binding read from an enclosing frame) and `error.rs` (the record of a name that stays dynamic). `ir::lower` calls it as it walks. |
| `ir` | `hir.rs` (the tree with names resolved and declared types attached), `mir.rs` (basic blocks, terminators, flattened temporaries), `lower.rs` (AST → HIR → MIR), `builder.rs` (blocks and temporaries) and `dump.rs`. `akk ir <file>` prints it; it does not execute anything yet, and a construct MIR does not model is refused with `E176` rather than approximated. |
| `opt` | `const_fold.rs` folds constant arithmetic, comparisons, `!`, unary minus, type tests on literals and constant branch conditions -- with the interpreter's own rules (two ints divided give a float, `%` is `rem_euclid`), and it leaves division by zero, overflow and mixed int/float pairs alone. `dce.rs` deletes unreachable blocks and unused definitions that cannot fail or have an effect. `akk ir --opt` prints the result next to the unoptimised MIR. `inline.rs` is not written yet. |
| `interp` | The tree-walking interpreter: the definition of what an Akkhara program means. One `mod.rs`; this is the reference implementation ("oracle") every other engine must match. Holds the value model, environments, builtins and the `eval_*_call` entry points for the compiled-in libraries. |
| `vm` | The bytecode engine used by `akk --vm`: `instr.rs` (instruction set), `compiler.rs` (tree -> bytecode), `vm.rs` (dispatch loop). A construct the compiler does not implement is refused with `E170` rather than silently differing from `interp`. |
| `sema` | `warnings.rs` builds the static warnings (`W001`-`W008`) and `check.rs` runs them, including `library_warnings` for `use`d and installed libraries. |
| `project` | `manifest.rs` reads `akk.toml`, `library.rs` resolves `use` to a neighbouring script, a dependency or an installed package, and `bundle.rs` implements the `akk build` format: a copy of `akk` with the program appended and CRC-checked. |
| `stdlib` | The seven libraries compiled into the binary: `app`, `file`, `json`, `random`, `request`, `server`, `time`. Each is a plain Rust module that does the work; `interp` owns the language-facing surface and converts at the boundary. |
| `diagnostics::codes` | The single registry of every `E###` / `W###` code, with a test that scans `src/` so a message cannot carry an unregistered code. |
| `text` | Grapheme-cluster `len`, indexing and slicing, so one Myanmar syllable counts as one character. |
| `backend` | The native code generator for one slice of the language: `x86_64/` has the instruction encoder (`encode.rs`), the refusal rules and type analysis (`mod.rs`) and MIR-to-machine-code (`gen.rs`); `object/elf.rs` and `object/pe.rs` write a static Linux or Windows executable byte by byte; `link/` writes the file and, on Linux, marks it executable. No assembler or linker is used, and a construct outside the slice is refused with `E177` before a byte is emitted. Every top-level `fn` becomes its own routine with its own frame, and calls between them follow the convention described below. Two strings compare with `==`/`!=` byte-wise and with an ordering lexicographically, through `rt_cmp_str`, and the numeric builtins `abs`, `min`, `max`, `floor`, `ceil` and `round` are emitted in line (an int `min`/`max` cannot mix with a float, because the result's kind would depend on the values; the rounding builtins convert their number to a double, round it and cast with the interpreter's saturating `as i64`). A builtin misused with the wrong kind or arity is refused with the interpreter's `E110`, not `E177`. Collection literals (`list`/`tuple`/`set`/`dict`) are emitted with indexing, `len` and collection printing through the runtime value layer; iterating a collection, storing into an element and comparing two collections are still refused. |
| `runtime` | The code linked into that executable: `write`/`exit` shims (`print.rs`, syscalls on Linux and `kernel32` on Windows), `rt_print_int` / `rt_print_bool` / `rt_print_str`, `rt_cmp_str` for lexicographic string comparison, `rt_panic_zero` for `%` by zero, which reports `E038` on stderr and exits 1 like the interpreter, and the collection runtime (`collections.rs`): a 256 KiB bump arena, descriptor indexing/printing, `E049`/`E053` with the lowered line, and an honest `E177` exit when the arena is exhausted. Values are one 64-bit word; a string is the address of `{ length, bytes }` in the constant pool, and a collection is a descriptor with explicit element tags. |
| `cli` | Every command, plus the terminal `ui` and `progress` helpers. |

## Native calling convention

Compiled programs may have any number of top-level `fn`s. `<main>` stays the
entry point and must take no parameters; it is emitted first, so its offset is
the image's entry point. Inside a call, the caller and the callee agree on:

| | |
| --- | --- |
| Arguments | The first six travel in `rdi`, `rsi`, `rdx`, `rcx`, `r8`, `r9`, in order. A function with more than six parameters, or a call with more than six arguments, is refused with `E177`. |
| Order of evaluation | Arguments are evaluated left to right, the order `interp` uses. Each result is stored in a scratch area the caller reserves (rounded to 16 bytes) and only then loaded into the argument registers, so a call nested inside an argument cannot disturb the registers still to be filled. |
| Result | `rax`. A function that returns nothing leaves whatever `return` last stored there; callers only read `rax` when the callee returns a value, which the analysis checks. |
| Callee-saved | `rbp` only: the prologue pushes it and the epilogue is `leave; ret`. Every other register is caller-saved, so the generated code keeps every live value in a frame slot and never relies on a register across a call. |
| Frame | One 64-bit slot per parameter, binding and temporary, at `[rbp - 8 * (n + 1)]`; the frame size is `align16(8 * slots)`, at least 16. Parameters are stored into their slots by the prologue. |
| Stack alignment | `rsp` is 16-byte aligned before every `call`: the prologue's `push rbp` plus a frame of a multiple of 16 keeps every deeper call aligned, and the scratch area is a multiple of 16 too. |
| Teardown | The callee restores `rsp` and `rbp` itself (`leave`); arguments live in registers, so the caller has nothing to pop. |

A value is one 64-bit word: an `int` or a `bool` is its value, a string is the
address of `{ length: u64, bytes }` in the constant pool, and a `float` is the
raw `f64` bit pattern, so the calling convention does not change for it -- the
same registers and slots carry a float, and arithmetic moves the word into
`xmm0`/`xmm1` with `movq` and back with `movq` before storing. `+ - *` on two
ints stays in general-purpose registers; `+ - * /` with a float on either side
(and `/` even for two ints, which the interpreter computes as a division of
doubles) runs on SSE (`cvtsi2sd` promotes an int operand, `addsd`, `subsd`,
`mulsd`, `divsd`), and both operands are promoted before the operation. Float
comparisons use `ucomisd`; the unordered (NaN) case is branched first, and
`==`/`!=` keep the interpreter's structural rule -- an int and a float are
never equal, and `-0.0 != 0.0` -- while ordering promotes a mixed pair.

At run time a division checks its divisor: `/` of two ints whose divisor is
zero jumps to the `E036` panic and a float division whose divisor is zero to
`E037`, matching the interpreter instead of letting the hardware trap or
produce an infinity. `%` stays integer-only and a float remainder is refused
with `E177`. Float parameters, returns and locals are one word like any other,
and a float result comes back in `rax` as its bits.

A raise (`ပစ်ပါ` / `throw`) and its `try/catch` clause are part of the slice.
A pending raise lives in three slots of the data section -- its identity, the
`E178 လိုင်း ...` prefix and the message -- so the `check_error` emitted after
every call can unwind it out of a callee into the caller. The clause is chosen
by comparing the identity the raise was given (interned strings make that a
pointer compare), `E178` takes every raise and `E` takes anything; a clause
that does not match runs its `finally` before passing the raise on, and a
matched one clears it before running its body. The message must be a string. A
clause for one of the interpreter's own codes (`E036` and the like) is refused
with `E177`, because a compiled program's panics exit instead of unwinding, and
so is a `break`, `continue` or `return` that would leave a `finally` without
running it. `<main>` reports a raise nothing caught through `rt_report_error`:
the interpreter's exact stderr line, then exit 1.

A failed `akk build` also leaves a diagnostics file holding the static
warnings the program has and the error that stopped the build:
`<file_name>_info.akop` beside the source for `akk build <file>`, and
`<output>_info.akop` beside the executable in `out_dir` for the project
build. A successful build writes only the executable and removes an info file
an earlier failed build left; both paths write the same format.

Printing a float is the interpreter's formatting, computed without floating
point: the runtime turns the exact binary value into decimal digits in a
base-10 bignum (`m * 5^-e` or `m * 2^e` depending on the exponent's sign), then
works out whether the value is integral (print the exact digits and `.0`, the
`{:.1}` form) or not, and in the latter case finds the shortest digit string
that round-trips to the same bits by testing candidate lengths 1..17 against
the rounding interval `(2m-1, 2m+1) * 2^(e-1)` with round-half-to-even. The
result is positional with no exponent (`0.0000001`), plus `inf`/`-inf`/`NaN`
and `-0.0`, exactly as the interpreter spells them.

The backend works out whether each temporary and binding is an int, float,
bool or string across the whole program (a call teaches its callee's parameter
kinds and its result kind, in both directions, until nothing changes) and
refuses with `E177` rather than guessing when a kind cannot be settled. Once
that fixpoint settles, anything still unlabelled in a numeric position is
`int` -- that is what a bare `a + b` means -- which is then fed back through
another round so the rest of the program can learn from it. A builtin's result
is never defaulted: `abs` keeps its argument's kind and `min`/`max` keep the
one kind their candidates share, so their kinds settle only once the arguments
do (which is why `fn low(a, b) { return min(a, b) }` learns float from a float
call site); `floor`/`ceil`/`round` always produce an `int`, whatever number
they are given.

## Scaffolding

These modules exist so the planned pipeline has a fixed shape. They compile
only where `src/main.rs` declares them, and none of them affects how a program
runs today.

| Module | Status |
| --- | --- |
| `types` per-type folders | `types/{int,uint,float,...}/` hold design notes only (`ops`, `lower`, `error`). The `Type` enum itself is real and used by the IR. |
| `opt` per-pass files | `inline.rs` is not written yet; `const_fold.rs` and `dce.rs` are. |
| `backend::x86_64::aarch64` | Not written: only x86-64 is a target today. |
| `backend::object::macho` | Planned: a note only; macOS needs an aarch64 backend first. |
| `runtime` allocation, strings, lists, dicts | Planned: the slice has no heap, so `print`, `write`, `exit` and panic are the whole runtime. MIR's `Items` / `Len` describe what such a runtime would have to provide. |
| `sema::infer`, `sema::error` | Planned: type inference and typed diagnostics. The interpreter is dynamically typed. |
| `diagnostics::span`, `diagnostics::render` | Planned: spans with columns and a single renderer. Messages currently carry a line and are formatted where they are produced. |

`backend` and `runtime` are declared in `src/main.rs` and hold real code: the
rows above are what is still missing inside them.

## Tests

- Unit tests live beside the code they test (`#[cfg(test)] mod tests`).
- `tests/cli.rs`, `tests/cli_project.rs` and `tests/bundle.rs` drive the built
  `akk` binary end to end; fixtures are under `tests/fixtures/`.
- `tests/native/main.rs` builds the programs in `tests/native/fixtures/` with
  `akk build`, runs the executables, and compares their stdout and exit status
  with the interpreter's -- twice each, with and without `--no-opt`. The
  fixtures cover arithmetic, branches, strings, calls through a two-argument
  function, self-recursion (`factorial`, `fib`), mutual recursion, a frame with
  several locals and prints around calls, arguments that are themselves
  calls, float literals and their exact spelling (`0.30000000000000004`), true
  division and mixed int/float arithmetic, float ordering and equality, floats
  through parameters and returns, string equality and ordering (including a
  proper prefix), and the numeric builtins `abs`/`min`/`max` over ints and
  floats, including a float `min` reached through a function's parameters,
  `floor`/`ceil`/`round` over positives, negatives, exact halves, an int
  argument, an int beyond 2^53 and a call through a parameter, and raising and
  catching (an identity clause, `E178`, `E`, and a raise from inside a function
  that crosses a clause which does not match it). It also checks
  that `%` by zero and a float division by zero panic with the interpreter's
  `E038` and `E037`, that an uncaught raise writes the interpreter's exact
  `E178` line (line number, identity and message) and exits 1, that a builtin
  misused with the wrong kind or arity is
  refused with the interpreter's `E110` (four cases), and that nine
  constructs outside the slice
  (a float remainder, `min` mixing an int and a float, list, tuple, dict, set,
  a closure, a class and a catch clause for an interpreter error code) are
  refused by `E177` in both build modes without
  leaving a file behind, while the interpreter runs each of them. It needs an x86-64 host to mean anything, and skips elsewhere.

`benchmarks/` does not exist yet: no harness is checked in.

```sh
cargo build
cargo test --all-targets
```
