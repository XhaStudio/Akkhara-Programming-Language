# Changelog

## 3.0.0-alpha.3

### Added

- Float remainder in compiled programs: `%` with a float on either side
  promotes both operands and computes the interpreter's `f64::rem_euclid` -- the
  truncated remainder made non-negative -- so `-7.5 % 2.0` is `0.5` and
  `7.5 % -2.0` is `1.5`. SSE has no remainder instruction, so the runtime's
  `rt_rem_float` uses the x87 `fprem` (repeated while C2 reports an incomplete
  reduction) and then adds `|divisor|` to a negative result; `-0.0`, NaN and a
  positive result pass through. A zero divisor writes the interpreter's `E039`
  and exits 1, a NaN divisor returns NaN, and `%=` works on float and int
  bindings alike. `tests/native/fixtures/remainder.akk` covers int, float and
  mixed remainders, the sign rules, a zero result, a call and both `%=` forms
  with and without `--no-opt`; `float_rem_zero` pins the panic.
- `floor`, `ceil` and `round` in compiled programs: each takes one number and
  returns an `int` exactly as the interpreter's `as i64` cast does, so a NaN is
  `0`, a value at or past `2^63` is `i64::MAX`, one below `-2^63` is `i64::MIN`,
  and an int argument is converted to a double first (so
  `floor(9007199254740993)` is `9007199254740992`, as the interpreter has it).
  Truncation is `cvttsd2si` after branching away the out-of-range values, and
  the fraction decides the adjustment (negative for `floor`, positive for
  `ceil`, half away from zero for `round`). A builtin called with the wrong
  kind or number of arguments is refused with the interpreter's own `E110` and
  wording, minus the line number compiled code cannot carry; `min`/`max` over
  one kind and the `E177` refusals for constructs outside the slice are
  unchanged. `sqrt` stays outside the slice because its `E111` message quotes
  the offending value. `tests/native/fixtures/rounding.akk` covers positives,
  negatives, exact halves, an int argument, an int beyond 2^53 and a call
  through a parameter, with and without `--no-opt`;
  `builtin_misuse_reports_the_interpreters_code` checks `E110` parity for four
  misuses.
- The numeric builtins `abs`, `min` and `max` in compiled programs. `abs(x)`
  keeps its argument's kind: an int is negated when it is negative, and a float
  has its sign bit cleared, so `abs(-0.0)` is `0.0` and a NaN stays a NaN.
  `min`/`max` take one to six candidates, evaluate them left to right (a call
  inside an argument runs in order), and keep the first, replacing it only on a
  strict comparison -- the interpreter's ordering treats a NaN pair as equal, so
  an unordered compare never replaces. Their result is the candidates' kind, so
  the slice requires one kind: `min(3, 1, 2)` is an int while `min(1, 2.5)` is
  refused with `E177` (the interpreter would return whichever won, and the
  slice settles kinds statically). A user function named `abs`/`min`/`max`
  still wins, as it does in the interpreter. `tests/native/fixtures/builtins.akk`
  covers all three over ints and floats, with and without `--no-opt`, and the
  refusal test covers the mixed `min`.
- String comparison in compiled programs: `akk build <file>` now compiles
  `==`, `!=`, `<`, `<=`, `>` and `>=` on two strings, comparing them the way the
  interpreter does -- byte-wise over the common prefix, and the shorter string
  is smaller when one is a proper prefix of the other. A new `rt_cmp_str`
  runtime routine returns -1, 0 or 1 and the generated comparison tests it
  against zero. Comparing a string with a non-string is still refused with
  `E177`. `tests/native/fixtures/strcmp.akk` covers all six operators,
  including a proper prefix, with and without `--no-opt`.
- Floats and true division in compiled programs: `akk build` now compiles
  float literals, float bindings, parameters and results, `+ - * /` (an int
  operand is promoted, and `/` of two ints divides instead of truncating),
  float comparisons and float printing. A float travels as its raw `f64` bits in
  the same one-word slot as any other value, so the calling convention is
  unchanged; the arithmetic moves the word into `xmm` registers and runs on SSE
  (`cvtsi2sd`, `addsd`, `subsd`, `mulsd`, `divsd`). `==`/`!=` keep the
  interpreter's structural rule (an int never equals a float, `-0.0` is not
  `0.0`) while ordering promotes a mixed pair. A zero divisor panics with the
  interpreter's code (`E036` for two ints, `E037` with a float) instead of
  trapping. Printing is
  byte-identical to the interpreter: the runtime renders the exact decimal
  digits from the bits in a base-10 bignum and picks the shortest string that
  round-trips, so `0.1 + 0.2` prints `0.30000000000000004`. The kind analysis
  can now learn `float` in both directions, and once it settles it defaults
  what is left to `int` before running another round, so `fn add(a, b) { return
  a + b }` still compiles without any type annotation.
- Functions in compiled programs: `akk build <file>` now compiles programs with
  any number of top-level `fn`s. Each function gets its own stack frame with a
  slot per parameter, binding and temporary, takes up to six arguments, returns
  a value in `rax`, and calls may be nested as arguments, refer to a function
  defined later in the file, recurse, or be mutually recursive. The convention
  is documented in `docs/ARCHITECTURE.md` (arguments in
  `rdi`/`rsi`/`rdx`/`rcx`/`r8`/`r9`, result in `rax`, only `rbp` callee-saved,
  `rsp` 16-byte aligned at every call, `leave; ret` teardown). The backend works
  out each parameter and result's kind across the whole program and refuses
  with `E177` when it cannot, instead of guessing. `tests/native/` covers calls,
  self-recursion, mutual recursion, several locals with interleaved prints and
  nested call arguments, with and without `--no-opt`.
- The slice is now decided before `opt` runs, so the optimiser cannot fold a
  construct the backend cannot emit (constant folding can turn a refused
  expression into a value) into something that then builds: `--opt` and
  `--no-opt` refuse the same programs with `E177`. A program the middle end will not model (`E176`,
  e.g. a class) is reported by `akk build` as `E177`, quoting the reason.
- `akk build <file>` writes a real native executable beside the source: no
  assembler and no linker are used, the x86-64 machine code is encoded directly
  and a static Linux executable (or a Windows `.exe` on Windows) is written byte
  by byte. The supported slice is `fn <main>()` over `int`, `bool`, string
  literals and `print`, with `+ - * %`, comparisons, branches and jumps; `--no-opt`
  builds without the MIR passes. Anything else -- floats, `/`, calls, closures,
  classes, lists, dicts, string comparison -- is refused with the new code `E177`
  naming the construct, and no file is written. The produced program prints what
  the interpreter prints and exits with the same status, including the `E038`
  panic for `%` by zero; compiled code carries no spans, so that message has no
  line number. `tests/native/` proves it by running the binaries.
- `akk ir --opt <file>`: the MIR is printed again after `opt::const_fold` and
  `opt::dce` ran, with a one-line summary of what changed. Folding follows the
  interpreter's arithmetic exactly (two ints divided give a float, `%` is
  `rem_euclid`) and skips division by zero, int overflow and mixed int/float
  pairs; elimination removes unreachable blocks and unused definitions that
  cannot fail or have an effect.
- `akk ir <file>`: the compiler's middle end can now be inspected. It prints the
  symbol table, the resolved HIR and the MIR basic blocks for a program without
  running it. Nothing executes the IR yet -- the interpreter is still the
  meaning of an Akkhara program -- and a construct MIR does not model is
  refused with the new code `E176` instead of being approximated.

### Fixed

- A statement after an `if` could fall into the `if` body: the block holding it
  was left without a terminator, and `Builder::finish` gave it a fall-through to
  the next *created* block, which is the body when the block was created before
  it. The end of a function body is now closed explicitly with `exit` (or
  `return`), so `akk ir` prints it and the native backend executes it correctly.

### Changed

- `akk run <file>` keeps its compiled file in a `build` folder beside the
  source (`build/<file_name>.exe`), created on demand, and runs that file
  instead of the source: a missing `build` folder, a missing executable, or an
  executable older than its inputs all compile first, then run. `Interpreted in
  ...` is no longer printed by any run.
- The `Compiling`/`Building`/`Finished` header is styled on a terminal: the
  words and the folder are bold, the version bold blue, every number bold cyan,
  and `Finished` bold green. A piped stream gets the same lines as plain text,
  so nothing that reads the output has to strip escape codes. `Compiled in ...`
  now comes before `Finished ...`.
- `akk --vm` now honours `--no-warn`. This is an intentional behaviour change:
  the flag used to be accepted but ignored on the bytecode path, because it was
  read from `env::args()` *after* being removed from the filtered argument
  list, so `akk --vm --no-warn prog.akk` still printed static warnings. Both
  global flags are now read once, before filtering, and every command receives
  the same value (see the comment in `src/cli/mod.rs`).

### Internal

- `types`, `scope` and `ir` are real modules now: the `Type` enum and its
  lookup table, a name resolver (bindings, frames, captures) and the HIR/MIR
  lowering with a dumper. `opt`, `backend` and `runtime` remain design notes and
  stay undeclared in `src/main.rs`.
- The source tree is split into modules (`frontend`, `interp`, `vm`, `sema`,
  `project`, `stdlib`, `diagnostics`, `text`, `cli`) with scaffolding for the
  planned compiler pipeline. No behaviour changes beyond the `--no-warn` fix
  above.
- The built-in libraries moved from `libraries/<name>/main.rs` to
  `src/stdlib/<name>/mod.rs`. `akk install` still downloads packages into a
  `libraries/` folder beside the executable.
- Documentation no longer links `benchmarks/`, `examples/` or `tests/native/`:
  those parts of the intended layout hold no content yet, so the README,
  `docs/ARCHITECTURE.md` and the stdlib READMEs say so instead of pointing at
  missing paths. The workspace is also `cargo fmt`-clean.
