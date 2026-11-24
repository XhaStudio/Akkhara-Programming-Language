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
| `ir` | `hir.rs` (the tree with names resolved and declared types attached), `mir.rs` (basic blocks, terminators, flattened temporaries), `lower.rs` (AST → HIR → MIR), `builder.rs` (blocks and temporaries) and `dump.rs`. `akk ir <file>` prints it; it does not execute anything yet, and a construct MIR does not model is refused with `E176` rather than approximated. A `switch` is lowered into one `Move` of its subject, one `Compare`/`Branch` per `case` and a jump per arm; that needs no new MIR instruction and no runtime helper, and a `break` inside an arm leaves through the switch's end block while a `continue` still reads the enclosing loop's target. A `/` or `%` is preceded by one `GuardZero` carrying the operator's line and the block a zero divisor unwinds to, so the check is a real control-flow edge the optimiser must keep. A string literal's `{name}` placeholders are lowered the way the interpreter expands them (`\{`/`\}` escapes kept literal, placeholders in source order) into `ToStr` pieces joined with `+`, so interpolation and `str(x)` share one conversion form. |
| `opt` | `const_fold.rs` folds constant arithmetic, comparisons, `!`, unary minus, type tests on literals and constant branch conditions -- with the interpreter's own rules (two ints divided give a float, `%` is `rem_euclid`), and it leaves division by zero, overflow and mixed int/float pairs alone. `dce.rs` deletes unreachable blocks and unused definitions that cannot fail or have an effect. `akk ir --opt` prints the result next to the unoptimised MIR. `inline.rs` is not written yet. |
| `interp` | The tree-walking interpreter: the definition of what an Akkhara program means. One `mod.rs`; this is the reference implementation ("oracle") every other engine must match. Holds the value model, environments, builtins and the `eval_*_call` entry points for the compiled-in libraries. |
| `vm` | The bytecode engine used by `akk --vm`: `instr.rs` (instruction set), `compiler.rs` (tree -> bytecode), `vm.rs` (dispatch loop). A construct the compiler does not implement is refused with `E170` rather than silently differing from `interp`. |
| `sema` | `warnings.rs` builds the static warnings (`W001`-`W008`) and `check.rs` runs them, including `library_warnings` for `use`d and installed libraries. |
| `project` | `manifest.rs` reads `akk.toml`, `library.rs` resolves `use` to a neighbouring script, a dependency or an installed package, and `bundle.rs` implements the `akk build` format: a copy of `akk` with the program appended and CRC-checked. |
| `stdlib` | The seven libraries compiled into the binary: `app`, `file`, `json`, `random`, `request`, `server`, `time`. Each is a plain Rust module that does the work; `interp` owns the language-facing surface and converts at the boundary. |
| `diagnostics::codes` | The single registry of every `E###` / `W###` code, with a test that scans `src/` so a message cannot carry an unregistered code. |
| `text` | Grapheme-cluster `len`, indexing and slicing, so one Myanmar syllable counts as one character. |
| `backend` | The native code generator for one slice of the language: `x86_64/` has the instruction encoder (`encode.rs`), the refusal rules and type analysis (`mod.rs`) and MIR-to-machine-code (`gen.rs`); `object/elf.rs` and `object/pe.rs` write a static Linux or Windows executable byte by byte; `link/` writes the file and, on Linux, marks it executable. No assembler or linker is used, and a construct outside the slice is refused with `E177` before a byte is emitted. Every top-level `fn` becomes its own routine with its own frame, and calls between them follow the convention described below. Two strings compare with `==`/`!=` byte-wise and with an ordering lexicographically, through `rt_cmp_str`, `+` joins two strings into a fresh arena string through `rt_concat_str` (`rep movsb` over both byte ranges), and the numeric builtins `abs`, `min`, `max`, `floor`, `ceil` and `round` are emitted in line, `sum` walks a collection's descriptor adding every element word as a double (the analyzer's element kind decides the int/float answer, and an int answer goes through the saturating `as i64` cast), `keys` is the `rt_items_dict` list a dict loop already builds, and `values`/`entries` copy the stored pairs through `rt_values_dict`/`rt_entries_dict` (an entry is a fresh two-element tuple) (an int `min`/`max` cannot mix with a float, because the result's kind would depend on the values; the rounding builtins convert their number to a double, round it and cast with the interpreter's saturating `as i64`). A builtin misused with the wrong kind or arity is refused with the interpreter's `E110`, not `E177`. `push`/`append` appends one value to the list a variable names through that same runtime write with the index set to the descriptor's length, replaces the binding with the returned descriptor (value semantics, so a second binding keeps its contents) and answers the new length. `reverse` answers a fresh descriptor from `rt_reverse_seq`: a list variable takes it back, which is the interpreter's in-place reversal, while a tuple or any other list is reversed as a value and its binding is left alone. `pop`, `insert` and `remove` build the same kind of copy (`rt_pop_seq`, `rt_insert_seq`, `rt_remove_seq`) and rebind, answering the dropped element or the new length; `rt_sort_seq` is a stable insertion sort that compares ints, floats or strings, picked by the element kind the analyzer settled. A runtime failure inside one of them (`E110` for an empty pop, `E049` for a position outside the list) is written without the interpreter's line clause, because a call carries no span. Text reverse, and a sort over any other element kind, are still refused because they need the Unicode tables or the interpreter's collection ordering (`E042`). Collection literals (`list`/`tuple`/`set`/`dict`) are emitted with indexing (at any nesting depth), `len`, collection printing, iteration, element stores and collection equality/ordering (ordering is the interpreter's `E042`) through the runtime value layer. A collection's element shape crosses calls: a parameter is typed from its call sites and its body, and a returned collection's shape reaches the caller, so an index into a call's result is typed; one parameter must settle to one shape and disagreeing call sites are refused. Iterating a number is the interpreter's auto-range, built by `rt_items_range_int` (a float bound is rounded up first), so `for i in n` walks the ints below `n`. A string whose text is known at compile time also supports `len`, indexing and iteration: `opt::strings` lowers them, using the interpreter's grapheme clusters, to an int constant or a list of clusters. A string that only exists at run time, and heterogeneous collections, are still refused. |
| `runtime` | The code linked into that executable: `write`/`exit` shims (`print.rs`, syscalls on Linux and `kernel32` on Windows), `rt_print_int` / `rt_print_bool` / `rt_print_str`, `rt_cmp_str` for lexicographic string comparison, `rt_concat_str` for joining two strings in the collection arena, `rt_str_int` / `rt_str_bool` / `rt_str_float` for `str(x)` (the float one reuses the print formatter and copies its bytes into an arena string when a `.data` flag is set), `rt_panic_zero` for a compound `%=` by zero (a plain `/` or `%` needs no runtime routine: its `GuardZero` raises the code itself), and the collection runtime (`collections.rs`): a 256 KiB bump arena, descriptor indexing/printing/reversing/popping/insertion/removal/sorting (`rt_reverse_seq`, `rt_pop_seq`, `rt_insert_seq`, `rt_remove_seq`, `rt_sort_seq`), `E049`/`E053` with the lowered line and the line-free variants a call site uses, and an honest `E177` exit when the arena is exhausted. Values are one 64-bit word; a string is the address of `{ length, bytes }` in the constant pool, and a collection is a descriptor with explicit element tags. |
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

Every `/` and `%` is preceded by `MirInst::GuardZero`, which checks its divisor
before the operation runs: an int divisor against zero, a float one against
`ucomisd` zero with the unordered (NaN) case skipped first. A zero divisor
raises the interpreter's code -- `E036` for two ints and `E037` with a float in
it for `/`, `E038` and `E039` likewise for `%` -- as a *raise*, so it travels
the same unwinding path: the identity slot holds the code, the payload is the
exact message including the operator's line, no prefix is set, and the check
jumps to the innermost enclosing handler (or the function's escape). So
`E036 ကို ဖမ်းပါ။` and `E ကို ဖမ်းပါ။` catch it, the clause binds the code
rather than a message, a `finally` runs, and an uncaught one prints the
interpreter's stderr line and exits 1. Keeping the check out of the arithmetic
itself also means the hardware never traps or produces an infinity. `%` of two
ints is the interpreter's Euclidean int remainder; a `%` with a float on either
side promotes both operands and calls the runtime's `rt_rem_float`, which
computes the same `rem_euclid` with the x87 `fprem` (repeating while C2 reports
an incomplete reduction, then adding `|divisor|` to a negative result), because
SSE has no remainder instruction. A compound remainder assignment (`x %= 0`)
has no guard of its own and keeps the older `rt_panic_zero`, which exits 1 with
`E038` and no line. Float
parameters, returns and locals are one word like any other,
and a float result comes back in `rax` as its bits.

A raise (`ပစ်ပါ` / `throw`) and its `try/catch` clause are part of the slice.
A pending error lives in three slots of the data section -- its identity, the
`E178 လိုင်း ...` prefix and the message -- so the `check_error` emitted after
every call can unwind it out of a callee into the caller; the identity slot is
the flag, non-zero for a raise and for a run-time panic alike. The clause is
chosen by comparing the identity (interned strings make that a pointer
compare), `E178` selects a raise only (a panic has no prefix), and `E` takes
anything; a clause that does not match runs its `finally` before passing the
error on, and a matched one clears it before running its body. What a clause
binds is a raise's message, or the code of a run-time panic (`E036`). The
message of a raise must be a string. A clause may name one of the interpreter's
own codes only when the program can actually produce it -- `E036`/`E037` when it
divides, `E038`/`E039` when it takes a remainder -- because a clause for a code
this program never raises could never match. A `break`, `continue` or `return`
that would leave a `finally` without running it is refused with `E177`.
`<main>` reports an error nothing caught through `rt_report_error`: the
interpreter's exact stderr line, then exit 1.

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
| `opt` per-pass files | `inline.rs` is not written yet; `const_fold.rs`, `dce.rs` and the string lowering in `strings.rs` are. |
| `backend::x86_64::aarch64` | Not written: only x86-64 is a target today. |
| `backend::object::macho` | Planned: a note only; macOS needs an aarch64 backend first. |
| `runtime` allocation and collection kinds | There is no general allocator and no GC: strings and collections come from one fixed bump arena, and exhausting it is reported as `E177`. A heterogeneous collection is compiled (every element keeps its own tag), but a read of one of its elements has no static kind and is refused; the collection kinds outside the slice are refused too. |
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
  proper prefix), a list grown with `push`/`append` (inside a `while` loop too,
  with a second binding keeping its own contents) and reversed with `reverse`
  (in place for a variable, as a value for a tuple and a literal),   `pop`, `insert`, `remove` and a stable `sort` over
  ints, floats and strings (with a dropped result still changing the list),
  `switch` over ints, strings, floats and bools (a default written first, only
  the first match, case expressions evaluated in written order, no
  fall-through, a `break` that stops the switch and not its loop, a `continue`
  that reaches that loop, and the Myanmar spelling), `str()` over ints, floats,
  bools and strings and `{…}` interpolation (with an escaped brace pair and a
  parameter filled by a string and by an int call site),   `sum` over a list, tuple, set and a parameter (int and float elements, an
  int list that saturates) and `keys` printed, indexed and iterated, a
  heterogeneous collection printed, counted, compared, widened by a store and
  a `push`, a nested widening, a set with a run-time member and a dict with a
  run-time key, `values()`/`entries()` printed, counted, indexed, iterated and
  read through a string-valued dict's entry or a list-valued dict's nested
  shape,
  that a zero divisor is catchable there exactly like a raise (a named code and
  `E` both take it, the clause binds the code, a matching clause still runs its
  `finally`, `E178` takes no panic, and an uncaught one prints the
  interpreter's line and exits 1), and the
  numeric builtins `abs`/`min`/`max` over ints and
  floats, including a float `min` reached through a function's parameters,
  `floor`/`ceil`/`round` over positives, negatives, exact halves, an int
  argument, an int beyond 2^53 and a call through a parameter, and raising and
  catching (an identity clause, `E178`, `E`, and a raise from inside a function
  that crosses a clause which does not match it). It also checks
  that `%` by zero, a float remainder by zero and a float division by zero
  panic with the interpreter's `E038`, `E039` and `E037`, that an uncaught
  raise writes the interpreter's exact
  `E178` line (line number, identity and message) and exits 1,   that a builtin
  misused with the wrong kind or arity is
  refused with the interpreter's `E110` (eight cases, including the `push`,
  `reverse`, `insert` and `sort` arities), and that eight constructs outside
  the slice
  (a read of a heterogeneous element, iteration over one, `pop` over one,
  `len`/index/iteration over a run-time string, two call sites that disagree
  over one parameter's shape, `min` mixing an int and a float, a closure, a
  class and a catch clause for an interpreter error code) are refused by
  `E177` in both build modes without
  leaving a file behind, while the interpreter runs each of them. It also pins
  the arena's limits: a literal larger than the 256 KiB arena reports `E177`
  and exits 1, while a set or dict literal with a run-time member or key
  rebuilt thousands of times allocates one descriptor per evaluation and
  finishes normally. It needs an x86-64 host to mean anything, and skips elsewhere.

`benchmarks/` does not exist yet: no harness is checked in.

```sh
cargo build
cargo test --all-targets
```
