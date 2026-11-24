# Akkhara (အက္ခရာ)

A programming language with Myanmar-language syntax, keywords, and error messages, interpreted by a Rust binary called `akk`.

## Build

```
cargo build --release
```

The binary is produced at `target/release/akk` (`akk.exe` on Windows).

## Tests

`cargo test` runs the CLI fixtures under `tests/fixtures/`, including Myanmar
and `eng` syntax, control flow, recursion, classes, collections, library
calls, lambdas, and the shorthand forms (ternary, `switch`, default
parameters, destructuring). Each fixture is a `Case` in `tests/cli.rs` with
the output it must produce, and `every_fixture_is_covered` fails when a
fixture on disk has no entry there.

## Run

```
akk myprogram.akk
```

or, before installing the command globally:

```
./target/release/akk myprogram.akk
```

A run prints no timer: the last stdout line is the program's own output (and
nothing extra is printed when it fails). The only timing line `akk` shows is
`Compiled in 0.003s`, printed when an executable is actually written — by
`akk build`, or by an `akk run` that had to compile. A built executable never
prints a timer.

Every run also saves its terminal transcript beside the program as
`<file_name>.akop` — running `hello.akk` writes `hello.akop` holding what the
program printed, then the error message when it fails (a program that fails
before printing anything leaves just the error). The file is written on every
run, success or failure, and each run overwrites it. Because `akk run` executes
the compiled file, its transcript is written beside that executable
(`build/main.akop` for `akk run src/main.akk`, and `build/<name>.akop` for a
project run).

### akk run

`akk run` runs the program's executable instead of re-reading the source every
time. It checks whether `<file_name>.exe` (or the project's `[build]` output)
is newer than everything the program is built from — the source, the scripts it
imports, `[dependencies]` paths and `akk.toml` — and:

- when it is current, runs it directly (no compile, no timer);
- when it is missing or stale, compiles it first (printing `Compiled in ...`),
then runs it.

A single file compiles into a `build` folder beside its source
(`build/<file_name>.exe`); a project uses its manifest's `[build] out_dir` and
`[build] output`. A missing `build` folder, a missing file, or a file older
than the source all take the same path: compile, then run. Bytecode is embedded
when the program can be carried that way, and the source is embedded instead
for a program the bytecode backend refuses (`E170`: `App`, `request`, script
imports), so both forms run identically. `akk file.akk` (without `run`) still
interprets the source directly and writes no executable.

### akk build

`akk build` writes one executable for the project in the current folder:

```
cd demo
akk build
.\build\demo.exe
```

`akk build` does nothing when the output is already newer than every input it
reads: it prints `Already compiled <path>` and exits 0. Otherwise it writes
the file and closes with `Compiled in 0.003s`.

The header a build and a run print, in order:

```
Compiling demo 0.1.0 (/path/to/demo)
Building [##########] 100% (3/3)
Compiled in 0.003s
Finished /path/to/demo/build/demo.exe (257814613 bytes)
```

On a terminal the words are styled -- `Compiling` and the folder name bold,
the version bold blue, every number bold cyan, and `Finished` bold green. `NO_COLOR=1` (or a `TERM=dumb`, or a piped stream) turns
all of it off and leaves the same text, so a script can match these lines
without stripping escape codes.

The output file is **a copy of the `akk` you ran, with the program appended
to the end of it**. It is not machine code, nothing is translated, and it does
not start or run any faster than `akk run` on the same source: a built program
only saves typing the command. Open the built file in a text editor and the
program's source is there, readable, after the interpreter's bytes — so a
built file is not a way to keep source secret.

When the file starts, the interpreter inside it finds the appended program and
runs it exactly as `akk run src/main.akk` would: same output, same error
messages, same exit code, and the same `<name>.akop` transcript, written
beside the executable (`build\demo.akop`). Every command-line argument is
ignored except `--no-warn` — `demo.exe update` runs the program, not the
updater. A built program makes no `libraries` folder and prints no
`Compiling`/`Building`/`Running` header.

What is carried inside the file:

- the entry file's source (from `[package] entry`, default `src/main.akk`);
- every `[dependencies] ... { path = ... }` script it uses;
- every neighbouring `<name>.akk` script its `use` statements reach, directly
  or through another script.

What is **not** carried inside:

- packages installed under `libraries\` (anything `akk install` put there).
  They are read from the `libraries` folder next to the built executable at
  run time, so copy that folder beside the executable if the program uses an
  installed library;
- `[build.bundle] include` files — not implemented yet, and a manifest with a
  non-empty `include` is refused instead of silently dropped;
- the source text, when `[build] mode = "bytecode"`: the program is compiled
  to bytecode first (`AKKBC001` plus the instruction stream) and the build
  embeds that instead of the `.akk` source. A construct the bytecode backend
  does not implement yet is refused at build time with `E170` rather than
  compiled into behaviour that differs from `akk run`.

At run time the file checks the appended program first: the length, the
header, and three CRC-32 checksums. A file that was truncated, edited or
partly copied prints an error and runs nothing.

Platforms: **Windows and Linux**. macOS and anything else are refused with
`E165`, because appending bytes to a signed arm64 binary would break its
signature.

Where the output goes: `[build] out_dir` (default `build`) under the project
root, named `[build] output` (default the package name) plus `.exe` on
Windows, or `[target.windows] output` on Windows when the manifest sets it.
The name must be one file name — no folder, no `..` — otherwise `E147`.
Building writes no run transcript (`.akop`) and does not run the program; a
failed build writes only `<output>_info.akop` (see below).

Errors `akk build` can report:

| Code | Meaning |
| --- | --- |
| `E159` | a built file is damaged (run time: trailer, header, length or CRC) |
| `E160` | a built file's format version or kind is not understood (run time) |
| `E161` | the slot was not found exactly once in the copy, or `akk` is itself a built file |
| `E163` | a `use` script was found but could not be read |
| `E164` | a build option is not implemented (`[build.bundle] include`) |
| `E170` | the program uses something `mode = "bytecode"` cannot compile yet |
| `E165` | bundles are not supported on this platform |
| `E168` | writing the output failed (creating `out_dir`, copying, writing, renaming) |
| `E177` | `akk build <file>` (native): the program uses a construct the native backend cannot compile |

Both forms of `akk build` leave a diagnostics file when compilation fails:
`akk build <file>` writes `<file_name>_info.akop` beside the source, and the
project build writes `<output>_info.akop` beside the executable in `out_dir`.
The file holds the static warnings the program has, then the error that
stopped the build (`E177`, a lexer/parser error, or a bytecode refusal such as
`E170`). A build that succeeds writes no info file and removes one left by an
earlier failure.

> Antivirus note: a program that copies its own executable and writes it to
> disk looks like a packer to some scanners, and a freshly built file may be
> quarantined. That is a false positive about the copying, not about the
> program; add an exclusion for the project folder if it happens.

## Performance

The interpreter is a Rust binary, and for anything with functions in it most of
a run's cost is in the call and loop paths rather than in lexing or parsing.

> **Not measured yet.** The benchmark programs and timing harness this section
> used to describe are not checked into this repository, so no numbers are
> published here and none can be reproduced from a clone. If you change the
> interpreter, time a real program before and after: `cargo build --release`,
> then time `target/release/akk prog.akk` against a copy of the previous
> binary. `akk` prints no run timer, so time the whole process -- on Windows a
> plain run also pays process startup.

## Install the `akk` command (Windows / PowerShell)

See the comment block at the top of `command.ps1` for full instructions —
short version: build once with `cargo build --release`, then add this line
to your PowerShell `$PROFILE`:

```powershell
function akk { & "C:\path\to\akkhara\command.ps1" @args }
```

Restart PowerShell and `akk file.akk` works from anywhere.

## Language reference

- Variable: `<name> သည် <value> ဖြစ်၏။` or the short `<name> က <value>။`
- Print: `<value> ကို ဖော်ပြပါ။` or `<value> ကို ပြပါ။`
- Input (discard): `<prompt> ကို မေးပါ။`
- Input (assign): `<name> အတွက် <prompt> ကို မေးပါ။`
- Type conversion: `စာ(<value>)`, `ကိန်းပြည့်(<value>)`, `ဒဿမကိန်း(<value>)`, `ဘူးလ်(<value>)` (or `str(...)`, `int(...)`, `float(...)`, `bool(...)` in `eng`)
- Math assignment: `<var or value> ကို <amount> <fn>ပါ။` or `<var> အတွက် <amount> <fn>ပါ။`
  (fn: `တိုး` increase, `လျော့` reduce, `မြှောက်` multiply, `စား` divide — the `အတွက်` form defaults an undeclared variable to `0` before applying the operation)
- Function call: `<fn> ကို လုပ်ပါ။` (no arguments),
  `<fn> ကို လုပ်ရန် <arg>[, <arg>] ဖြင့်။`, or with the verb at the end:
  `<fn> ကို (<arg>[, <arg>]) ဖြင့် လုပ်ပါ။` — the same two spellings work for
  library functions as `<lib> ၏ <fn> ...`
- Function definition: `လုပ်ငန်း <name> သည်` ... `ပြီး။` (no parameters),
  `လုပ်ငန်း <name> အတွက် <p1>, <p2> ဖြင့်` ... `ပြီး။`, or a parenthesized header:
  `လုပ်ငန်း <name>() သည်` ... `ပြီး။` / `လုပ်ငန်း <name>(<p1>, <p2>) ဖြင့်` ... `ပြီး။`.
  Either parameter list may give trailing parameters defaults, the same
  `name = <value>` spelling an eng `fn` header takes:
  `လုပ်ငန်း greet(name, punct = "!") သည်` ... `ပြီး။` takes one argument or two.
- Function call, bare spelling: `<fn>()` / `<fn>(<arg>[, <arg>])` -- the same call
  as `<fn> ကို လုပ်ပါ။`; a trailing `;` or `။` is optional, and one of them is
  needed when another statement follows (`greet();` or `greet()။`).
- Collections: `<name> မှာ <literal> ဖြစ်၏။`
  - List: `[1, 2, 3]`
  - Tuple: `(1, 2, 3)`
  - Set: `{1, 2, 3}` (duplicates are dropped)
  - Dict: `{ "key" သည် value ဖြစ်၏။ ... }` or the short `{ "key" က value။ ... }` (each entry can span multiple lines); also declarable as `အဘိဓာန် <name> က <literal>။`
  - Table: just a list of lists, e.g. `[["a","b"], ["c","d"]]`
  - Full reference, in Myanmar, with runnable examples: [`docs/COLLECTIONS.md`](docs/COLLECTIONS.md)
- Statements end at their terminator: `။` in the Myanmar spelling and `;` in
  the eng one. A line break never ends a statement, so several statements may
  share a line (`a = 1; b = 2;`) and one statement may span as many lines as
  it needs.
- Comments: `# ...` (to the end of the line)
- Arithmetic: `+ - * /` (Myanmar names in errors: ပေါင်း၊ နှတ်၊ မြှောက်၊ စား), unary minus supported (`-1`, `value * -1`, `value ကို -1 မြှောက်ပါ။`)
  - Precedence, tightest first: `**` (exponent) → `* / % \` → `+ -` → comparisons. Operators of the same tier group left to right, so `1 + 2 * 3` is `7` and `10 - 4 - 3` is `3`; parentheses override (`(1 + 2) * 3` is `9`).
- Numbers: both Myanmar digits (၀-၉) and ASCII digits
- Booleans: `True`/`False` and `မှန်`/`မှား`

### Blocks

A block is a header with no `။` of its own, its statements, and a closing
`ပြီး။`. If and while have a long and a short spelling; both parse to the
same block.

- If: `အကယ်၍ (<condition>) ဖြစ်လျှင်` ... `သို့မဟုတ် (<condition>) ဖြစ်လျှင်` ... `မဟုတ်လျှင်` ... `ပြီး။`.
  Leave the leading keyword out and shorten the terminators instead:
  `(<condition>) ဖြစ်ရင်` ... `အခြား (<condition>) ဖြစ်ရင်` ... `မဟုတ်ရင်` ... `ပြီး။`.
  `မဖြစ်လျှင်` / `မဖြစ်ရင်` runs the branch when the
  condition is *not* true.
  A one-statement guard needs no `ပြီး။` at all: let the statement follow the
  header's own terminator, on the same line --
  `(age > 18) ဖြစ်ရင်။ "x" ကို ပြပါ။` (`(age > 18) မဖြစ်ရင်။ ...` guards the
  other way round).
- While: `အခြေအနေ (<condition>) ဖြစ်နေစဉ်` ... `ပြီး။`, or
  `(<condition>) ဖြစ်နေစဥ်` ... `ပြီး။` -- the keyword is optional, and
  `မဖြစ်နေစဉ်` / `မဖြစ်နေစဥ်` loops while the condition is not true.
- For, over a collection: `x သည် xs ထဲ တစ်ခုစီ` ... `ပြီး။` -- `ထဲ` is the
  short particle (`x သည် xs ထဲမှ တစ်ခုစီ` is the longer spelling of the same
  header), and the `အတွက်` spelling `x အတွက် တစ်ခုစီ(xs)` ... `ပြီး။` still
  works. A dict yields its keys.
- For, over a range: `x သည် တစ်ခုစီ(0, 10, 2)` ... `ပြီး။` (also
  `x အတွက် တစ်ခုစီ(...)`) -- the end is exclusive; the step may be left out
  (`တစ်ခုစီ(0, 10)` steps by 1), may be written with its sign
  (`တစ်ခုစီ(1, 2, +1)`), and a negative step counts down
  (`တစ်ခုစီ(10, 0, -1)`). One number ranges from 0: `တစ်ခုစီ(10)` runs 0..9.
  The step can follow a collection-form header too:
  `i သည် 0, 10 ထဲ တစ်ခုစီ 2 တိုးခြင်းဖြင့်` ... `ပြီး။`.

Conditions join with `and` / `or`, or with their own Myanmar groups
`(နှင့်)` / `(သို့)`:

```
(<cond1>) and (<cond2>) ဖြစ်ရင်
    ...
အခြား (<cond1>) (သို့) (<cond2>) ဖြစ်ရင်
    ...
မဟုတ်ရင်
    ...
ပြီး။
```

### Errors you raise (`ပစ်ပါ` / `throw`)

A program can fail on purpose, naming the failure and carrying a message;
`စမ်းရန် ... ဖမ်းပါ ... နောက်ဆုံးတွင် ...` catches it exactly like a runtime
error.

```
စမ်းရန်
    "လက်ကျန်ငွေ မလုံလောက်ပါ" ကို ငွေမလုံလောက်မှု အမည်ဖြင့် ပစ်ပါ။
ငွေမလုံလောက်မှု ကို ဖမ်းပါ။
    print(ငွေမလုံလောက်မှု);      # the message the raise carried
နောက်ဆုံးတွင်
    print("finally");
ပြီး။
```

- Raise: `<value> ကို <name> အမည်ဖြင့် ပစ်ပါ။`, or in eng spelling
  `throw <value> as <name>;`. The value is the message and may be any value —
  text, a number, a list, a dict — and it is the text the catch clause reads.
- Catch: `<name> ကို ဖမ်းပါ။` selects a raise by that name, or by its shared
  code `E178 ကို ဖမ်းပါ။`; `E ကို ဖမ်းပါ။` still catches everything.
- The catch name is also the variable the error is bound to: a raise binds the
  message it carried, every other runtime error binds its code (`E030`).
- A `finally` block runs whether the raise was caught or not, and a raise from
  inside a function call unwinds to whichever enclosing `try` selects it.
- An uncaught raise exits nonzero and writes
  `E178 လိုင်း <line> တွင် [<name>] <message>` to stderr, and the run's `.akop`
  transcript records it like any other error.
- `akk build` (a native executable) supports raising and `try/catch` too. Its
  message must be text there, because the catch clause binds it and an uncaught
  raise prints it; a raise inside a call unwinds to the clause that selects it,
  exactly as it does when interpreted. A zero divisor for `/` or `%` is a
  runtime error there too -- `E036`/`E037` for a division, `E038`/`E039` for a
  remainder, chosen by the operand kinds -- and it unwinds like a raise: a
  clause for that code catches it and binds the code, a `finally` still runs,
  `E178` does not take it, and an uncaught one writes the interpreter's exact
  line (line number included) and exits 1. A `break`, `continue` or `return`
  that would leave a `finally` without running it is refused with `E177` rather
  than skipping the block.
- `akk build` supports `switch` in both spellings. The subject is evaluated
  once, then the case expressions in written order until one is equal to it,
  so a later case is never evaluated and its side effects never happen; the
  first matching arm runs and nothing falls through, and the `default` arm
  runs only when no case matched, wherever it is written. A `break` inside an
  arm stops the switch rather than the loop it sits in, while a `continue`
  still reaches that loop. Equality is the language's `==`, so a float subject
  never equals an `int` case.
- `akk build` supports `str(x)` (also spelled `စာ` and `စာသား`) for an int, a
  float, a bool and a string, and `{name}` interpolation inside a string
  literal. The conversion is the interpreter's display form exactly -- `3.0`
  for a whole float, `2.5`, `True`/`False`, a string unchanged -- and
  interpolation is that conversion joined left to right with `+`, so
  `"v = {x}"` and `"v = " + str(x)` print the same text. `\{` and `\}` are
  literal braces, and a placeholder is resolved where the literal is written.
  A collection's `repr`, a `{အတု …}` self-field placeholder and a name that is
  not in scope are refused with `E177` rather than printed differently.
- `akk build` supports list/tuple/set/dict literals, indexing, `len`,
  collection printing, `for` over a list, tuple or set (a dict yields its
  keys, in insertion order) or over a number (the interpreter's auto-range:
  `for i in 5` walks 0 to 4, a float bound is rounded up, and zero or a
  negative bound runs no iterations), and writing through an index (`xs[i] = v`: replace
  at a position, append just past the end, update or add a dict key, and the
  interpreter's set duplicate rule). `sum` totals a list, tuple or set of
  numbers (a double accumulation, answered as an int unless a float element
  appeared), and `keys` answers a dict's keys in insertion order as a list;
  `values` and `entries` answer the same list of a dict's values or of its
  `(key, value)` tuples, built from the tags the dict already stores.
  `push`/`append` appends one value to the
  list a variable names and answers the new length, and `reverse` answers the
  elements in the opposite order: a list variable is reversed in place, while a
  tuple or a list literal is reversed as a value (text reverse needs the
  Unicode tables and is still refused). `pop`, `insert`, `remove` and `sort`
  work on a list variable too: `pop`/`remove` answer the element they dropped,
  `insert` answers the new length, and `sort` is a stable ascending sort over
  ints, floats or strings. `slice(xs, start[, end])` answers a fresh list of
  the elements between two positions, with both ends clamped to the length and
  a start past the end empty, exactly as the interpreter has it; a tuple or a
  set slices into a list, and a negative bound is its `E110`. Popping an empty
  list and a position outside the list are the interpreter's `E110`/`E049`,
  without the line clause a compiled call cannot carry. A write builds a fresh collection and
  rebinds the name, so another binding keeps its own contents, exactly as the
  interpreter's value semantics do. Equality (`==`/`!=`) is structural and
  order-sensitive, like the printed form: same kind, length and elements, and
  a dict compares its keys and values in insertion order. A collection of
  collections works at any depth: `grid[0][1]`, `deep[0][1][0]`, a list inside
  a dict, a `for` over rows and then over cells, and printing or comparing the
  nested value all type from a per-binding element shape. That shape crosses
  a call too: a list, tuple, set or dict parameter is typed from what its
  call sites pass and from what its body does with it, and a returned
  collection's shape reaches the caller, so indexing a call's result works.
  A collection whose elements are not all one kind (`[1, "two", 3.5, True]`)
  is compiled: every element keeps its own tag, so printing, `len`, equality,
  copying, an element store and `push` all work, and a store of another kind
  widens the collection instead of being refused (a list of int lists takes a
  list of strings). A set built from a run-time member and a dict built from a
  run-time key are deduplicated by the runtime, in the interpreter's order.
  A *read* of such an element keeps the tag the descriptor stored, so printing
  it (top level, through a binding, a parameter or a new literal) dispatches at
  run time; a use that needs one static kind -- `str`, arithmetic, iterating
  the collection, `pop`/`remove`/`sort`/`sum` -- is refused with `E177`.
  One function's parameter has one element shape; two call sites that pass
  different shapes are refused with `E177`. Ordering two
  collections is the interpreter's `E042`. A string whose text is known at
  compile time (a literal, or a binding assigned one once) also supports
  `len(s)`, `for ch in s` and `s[i]`, counted in grapheme clusters by the
  interpreter's own rules: these are lowered to an int constant or a list of
  clusters, so an out-of-range read is still the interpreter's `E049`.
  `+` joins two strings (the interpreter's only string arithmetic): a
  constant pair is folded, and a run-time join copies both into a new string
  in the runtime arena. Writing into a string, and `str` or arithmetic on a
  value whose kind varies, are still refused with `E177`, and so are `len`,
  indexing and iteration over a string
  that only arrives at run time (a parameter, a call result), because the
  native runtime carries no Unicode tables; a nested write such as
  `grid[0][1] = v` stays a parse error in the language itself. Collections
  use a fixed runtime arena; exhausting it reports `E177` on stderr and exits
  1.

### Type keywords
`စာသား`(str) `ကိန်းပြည့်`(int) `ဒဿမ`/`ဒဿမကိန်း`(float) `မှန်/မှား`(bool) `စာရင်း`(list) `အစု`(tuple) `အုပ်စု`(set) `အဘိဓာန်`(dict) `ဇယား`(table)

### Libraries

Import a library, then reach its functions with the `၏` particle.

- Import: `နည်းပညာများ <lib>[, <lib>] ကို အသုံးပြုပါ။`
  The `ကို` particle is required: writing `နည်းပညာများ <lib> အသုံးပြုပါ။` fails with
  `E094 လိုင်း <line> တွင် နည်းပညာ(<lib>) ကို အသုံးပြုရန် "ကို" ခံရေးရန် လိုအပ်ပါသည်`.
- Call (no arguments): `<lib> ၏ <fn> ကို လုပ်ပါ။`
- Call with arguments in parens: `<lib> ၏ <fn>(<arg>) ကို လုပ်ပါ။`
- Call with `လုပ်ရန်`/`ဖြင့်`: `<lib> ၏ <fn> ကို လုပ်ရန် <arg> ဖြင့်။`
  (the argument(s) may also be parenthesized: `<lib> ၏ <fn> ကို လုပ်ရန် (<arg>) ဖြင့်။`)
- Call with the verb at the end: `<lib> ၏ <fn> ကို (<arg>) ဖြင့် လုပ်ပါ။`
- Store the result in a variable: `<var> အတွက် <lib> ၏ <fn>(<arg>) ကို လုပ်ပါ။`
  or `<var> အတွက် <lib> ၏ <fn> ကို လုပ်ရန် <arg> ဖြင့်။`

Multiple arguments are comma-separated; each form accepts either spelling of
the argument list (`<a>, <b>` or `(<a>, <b>)`).

Built-in libraries and the functions they expose:

| Library | Function | Arguments | Result |
|---|---|---|---|
| `request` | `get` | `(url)` | response object |
| `request` | `post` | `(url, data)` | response object |
| `ကျပန်း` | `ကိန်း` / `random_int` | `(min, max)` | integer |
| `ကျပန်း` | `ဒဿမ` / `random_float` | `(min, max)` | float |
| `အချိန်` | `စောင့်` / `wait` | `(seconds)` | none |
| `App` | `screen` / `label` / `button` / ... | see below | window / widget handle (GUI) |

```
နည်းပညာများ request ကို အသုံးပြုပါ။

response အတွက် request ၏ get("https://api.example.com/data") ကို လုပ်ပါ။
response ၏ အခြေအနေကုဒ် ကို ဖော်ပြပါ။
```

A field of an object value can be read with `၏` or with `.` — `response ၏
အခြေအနေကုဒ်` and `response.အခြေအနေကုဒ်` are the same read, and dot
access chains (`p.inner.v`) and works on class objects too (`p.name`). A `.`
after a name that is *not* a variable is still a library call, so
`ကျပန်း.ကိန်း(1, 10)` keeps its meaning.

#### App — windows, widgets and an event loop

`App` is the GUI library, compiled into `akk` like the others: a window, its
widgets, dialogs and a drawing canvas, with no extra runtime needed. Build
the whole UI first, then `App.run(w)` shows the window and waits for it to
close — Tkinter's `mainloop()`.

Every `screen`/widget call returns a handle (an object value, exactly like a
`request` response), and a button's callback is the *name* of an Akkhara
function, given as text.

| Group | Functions |
|---|---|
| window | `screen(width, height)`, `title(w, text)`, `run(w)`, `close(w)` |
| widgets | `label(w, text, x, y)`, `button(w, text, x, y, "fn")`, `input(w, x, y, width)`, `textarea(w, x, y, width, height)`, `checkbox(w, text, x, y)`, `choice(w, [items], x, y)`, `listbox(w, [items], x, y, width, height)`, `table(w, [headers], [[cells], ...], x, y, width, height)`, `image(w, path, x, y)`, `canvas(w, x, y, width, height)` |
| values | `get(widget)`, `set(widget, value)`, `number(widget)` (reads a box's text as a number), `items(widget)`, `set_items(widget, list)`, `cell(table, row, col)`, `set_cell(table, row, col, value)` |
| geometry & style | `move`, `size`, `color`, `font`, `show`, `hide`, `enable`, `disable` |
| events | `on_key(w, "Enter", "fn")`, `every(w, milliseconds, "fn")` |
| dialogs | `message(text)`, `ask(text)`, `pick_file()` |
| canvas | `rect`, `circle`, `line`, `text`, `clear` |

```
နည်းပညာများ App ကို အသုံးပြုပါ။

w အတွက် App ၏ screen(320, 180) ကို လုပ်ပါ။
App ၏ title(w, "Greeting") ကို လုပ်ပါ။
box အတွက် App ၏ input(w, 20, 20, 200) ကို လုပ်ပါ။
ok အတွက် App ၏ button(w, "Greet", 20, 60, "on_greet") ကို လုပ်ပါ။
msg အတွက် App ၏ label(w, "type your name", 20, 110) ကို လုပ်ပါ။

fn on_greet() {
    name :str = App.get(box);
    App.set(msg, name);
}

App ၏ run(w) ကို လုပ်ပါ။
```

The eng spelling is the same names with dots — `w = App.screen(320, 180);`,
`App.set(msg, "hi");`, `App.run(w);`. The full reference (colours, key
names, font handling, canvas drawing and the `E120`–`E128` error codes) is in
[`src/stdlib/app/README.md`](src/stdlib/app/README.md).

#### Connecting another script

`နည်းပညာများ <name> ကို အသုံးပြုပါ။` resolves `<name>` in three steps, and
reports `E062` only when all of them miss:

1. a built-in library (`request`, `ကျပန်း`, `အချိန်`),
2. a package installed with `akk install <name>`, then
3. a plain `<name>.akk` script sitting next to the program being run (or in
   the current working directory, then beside the `akk` binary).

So a program can borrow helpers straight out of a neighboring file -- no
install step and no recompiling `akk` needed:

```
# Tools.akk
fn add(a, b) {
    return a + b;
}
```

```
# main.akk
နည်းပညာများ Tools ကို အသုံးပြုပါ။

TOOL :int = Tools.add(1, 100);      # = Tools ၏ add(1, 100)
print(TOOL);                        # 101
```

The imported script's functions are reached through its library name
(`<name> ၏ <fn>` or `<name>.<fn>`), and only the functions that script
defines are visible that way. Adding `အဖြစ် <alias>` to the import line gives
the script a shorter name -- `နည်းပညာများ ExpensesStore အဖြစ် Store ကို
အသုံးပြုပါ။` later reaches its functions as `Store.<fn>`. A script that
imports itself -- directly, or through a cycle of other scripts -- is
reported as `E119` instead of recursing forever.

The sample programs for this section -- `app_expenses.akk`, and the eight-slot
`ExpensesStore.akk` ledger it imports under the shorter name -- are not checked
into this repository yet, so there is no file to link here.

All error messages are in Myanmar, formatted as `လိုင်း <N> ...` (the `eng`
syntax below reports its own errors in English instead).

Five syntax errors also show the line they came from, with a caret where the
problem is: a declaration with no value (`x က ။`, `E001`), a statement whose
`။`/`;` terminator is missing (`E002`), a string with no closing `"` (`E003`),
brackets that do not pair (`E004`), a print/input call whose `ကို` is missing
(`E006`), and one written with no value to act on (`E007`):

```
E002 လိုင်း 1 ၏ စာကြောင်းအဆုံးတွင် '။' မရှိပါ။
line 1| "hi" ကို ဖော်ပြပါ
                  ↑
                  |
                  |
        `။` မပိတ်ထား
```

```
E003 လိုင်း 1 တွင် string တန်ဖိုးအတွက် ပိတ် '"' မရှိပါ။
line 1| "hello
              ↑
              |
              |
        `"` မပိတ်ထား
```

```
E004 လိုင်း 1 တွင် ကွင်းများ ({}/[]/()) မှန်ကန်စွာ မတွဲထားပါ။
line 1| (]
        ↑↑
        ||
        ||
        ကွင်းများမှန်ကန်စွာ မတွဲထားပါ။
```

```
E006 လိုင်း 1၌ "ဖော်ပြပါ။" လုပ်ဆောင်ရန် "ဖော်ပြပါ။" ၏အရှေ့၌ "ကို" ခံရေးရန်လိုအပ်သည်။

line 1| "hi" ဖော်ပြပါ။
            ↑
            |
            |
        "ကို" ခံရေးရန်
```

A bracket left without its closing partner (`[1, 2` with no `]`) is `E003` and
names the bracket that is still open.

### eng — English (C-style) syntax

The `eng` spelling is built into the core of `akk` — **no import line is
needed** — and can be mixed freely with the Myanmar forms in the same file.
Its types are `int`, `float`, `str` and `bool`.

```
name :str = "John";           # typed declaration
age :int = 25;
pin max_users :int = 100;     # constant — cannot be reassigned
print(name);                  # print
input("Press enter: ");       # read a line, discard it
answer :int = input("n? ");   # read a line, coerced to :int
name = "Jane";                # assignment (a first one declares the variable)

if (age >= 20) {              # the parentheses around a condition are optional
    print("adult");
} else if (age >= 13) {
    print("teen");
} else {
    print("child");
}

count :int = 0;
while (count < 3) {
    count += 1;                # compound assignment: += -= *= /= %= ^=
}

loop {                         # infinite loop until a `break`
    count += 1;
    if (count == 10) {
        break count;           # `break <value>` also works
    }
}

fn add(a, b) -> int {          # parameter/return type annotations are optional
    return a + b;
}
fn fib(n) {
    if (n < 2) { return n; }
    return fib(n - 1) + fib(n - 2);
}
print(add(2, 3));
```

A `loop` can also be the value of a declaration: the variable is seeded with
its type's default (`0` / `0.0` / `""` / `false`) before the loop runs, so the
body can accumulate into it, and it takes the value passed to `break`.

```
total :int = loop{
    total += 3;
    if (total > 10){
        break total
    }
}
print(total);        # 12
```

`break` is spelled `ရပ်ပါ။` in the Myanmar syntax (`<value> ကို ရပ်ပါ။`
breaks with a value), and it stops the innermost loop — `while`, `for` and
`loop` alike. A `break` (or a `return` / a `break`) written as the last
statement of a block may drop its `;`, e.g. `if (i == 3) { break }`.

An eng statement has to end with its `;`; leaving it off reports
`E002 Line <line>: Its missing semicolon ";"`, whether the line ends with nothing at all or with a Myanmar `။`.

Conditions compare with `== != < <= > >=` and combine with `&&`, `||` and `!`,
e.g. `if (flag && !done) { ... }`. The same operators work wherever a value
is expected — `ok :bool = a > 1 && b < 2;`, `ok = !done;` — with `&&` binding
tighter than `||` and short-circuit evaluation, so
`0 < len(xs) && xs[0] > 0` is safe on an empty list. Both operands must be
`မှန်/မှား`; anything else reports `E043`.

Membership is `in` -- `if (x in xs) { ... }`, `ok = x in xs;` -- the same test
`contains(xs, x)` makes (a substring of a string, an item of a
list/tuple/set, a key of a dict). The Myanmar spelling puts the collection
first: `(xs ထဲ x) ဖြစ်ရင်။ "x" ကို ပြပါ။`.

A guard can also carry its `if` at the end, so a one-line conditional needs no
braces:

```
print("x") if (age > 18);
```

Builtin helpers (core syntax — usable as statements or inside expressions):

| Builtin | Arguments | Result |
|---|---|---|
| `len(x)` | string or collection | length (characters / items) |
| `abs(x)` | number | absolute value |
| `min(...)` / `max(...)` | several numbers/strings, or one collection | smallest / largest |
| `sqrt(x)` | number `>= 0` | square root (float) |
| `floor(x)` / `ceil(x)` / `round(x)` | number | integer |
| `upper(s)` / `lower(s)` / `trim(s)` | string | new string |
| `contains(hay, needle)` | string or collection, and a value | `True` / `False` |

#### Anonymous functions (`fn(x) { ... }`)

`fn(<params>) { <body> }` written where a value is expected is a function
value — the same thing `fn <name>(...) { ... }` defines, without the name. It
is core syntax, so the Myanmar spellings can use one too.

`<name> => <expression>` (or `(<p1>, <p2>) => <expression>`) is that same
value on one line: `map(xs, x => x * 2)` is `map(xs, fn(x) { x * 2 })`, and
`reduce(xs, (a, b) => a + b, 0)` takes two parameters. Both spellings of the
language use the arrow, so a Myanmar sentence can pass one to `map` just as
easily. The arrow form takes plain parameter names (no defaults) and its body
is a single expression.

```
xs = [1, 2, 3, 4, 5];
print(map(xs, fn(x) { x * 2 }));              # [2, 4, 6, 8, 10]
print(filter(xs, fn(x) { x % 2 == 0 }));      # [2, 4]
print(reduce(xs, fn(a, b) { a + b }, 0));     # 15

# the callback can also be stored first, and it closes over its scope
factor :int = 10;
scale = fn(x) { x * factor };
print(map(xs, scale));                        # [10, 20, 30, 40, 50]

adder = fn(n) { fn(x) { x + n } };            # a lambda may return one
add5 = adder(5);
print(add5(10));                              # 15
```

- The body's last expression is its result; earlier statements run first, and
  `return` works inside a body like it does in a named function.
- A lambda takes as many arguments as its parameter list declares (a
  mismatch is `E033`, worded like any other function call).
- `map`, `filter` and `reduce` accept, in the callback position, a lambda, a
  named function, a name bound to a function value, or a library function —
  `map(xs, Tools.add)` is the same call as `fn(x) { Tools.add(x) }` (the
  library has to be imported, and the function's own arity still applies;
  one that returns nothing is `E117`).

```
xs သည် [1, 2, 3] ဖြစ်၏။
ရလဒ် သည် map(xs, fn(x) { x * x }) ဖြစ်၏။
ရလဒ် ကို ဖော်ပြပါ။
```

#### Ternary, switch, default parameters, destructuring

```
n = 7;
print(n % 2 == 0 ? "even" : "odd");     # a value, not an if/else block

fn greet(name, punct = "!") {            # trailing parameters may be omitted
    return "Hi, " + name + punct;
}
print(greet("Su"));                       # Hi, Su!
print(greet("Su", "?"));                  # Hi, Su?

[a, b] = [1, 2];                          # one name per item, in order
print(a + b);

switch (n) {
    case 1: print("one");
    case 7:
        print("seven");
    default: print("other");              # only when no case matched
}
```

- **Ternary** — `<cond> ? <then> : <else>`, or the Myanmar spelling
  `(<cond>) ဖြစ်ရင် <then> မဟုတ်ရင် <else>` written where a value is expected:
  `(n % 2 == 0 ဖြစ်ရင် "စုံ" မဟုတ်ရင် "မ") ကို ပြပါ။` `<cond>` must be
  `မှန်/မှား` (`E043` otherwise), only the chosen branch is evaluated, it
  binds looser than `||`, and it associates to the right
  (`a ? b : c ? d : e` is `a ? b : (c ? d : e)`). A missing `:` is `E151`;
  the Myanmar form's missing `မဟုတ်ရင်` is `E157`.
- **switch** — `switch (<value>) { case <value>: <statements> default:
  <statements> }`, or the Myanmar spelling
  `ရွေးရန် (<value>) ... ပြီး။` with `<value> ဖြစ်ရင် <statements>` arms and a
  `မူလ <statements>` default. Cases are tried in order and only the first
  match runs, so there is no fall-through and no `break` is needed; `default`
  runs when none matched, wherever it is written. `break` inside an arm stops
  the switch. A malformed switch is `E153` (`E156` in the Myanmar spelling).
- **Default parameters** — `fn f(a, b = 1, c = b * 2)`, and the same
  `name = <value>` on a Myanmar `လုပ်ငန်း` header. Defaults must be the
  trailing parameters (a required one after a default is `E109`), a call may
  supply any number of arguments from the required count up to the full list
  (`E033` otherwise), and each default is evaluated in the call's own frame,
  so it can read the parameters before it. Lambdas take them too:
  `fn(x, times = 2) { x * times }`.
- **Destructuring** — `[a, b] = <list or tuple>;` (also `(a, b) = ...`), or
  the Myanmar spelling `[a, b] သည် <list or tuple> ဖြစ်၏။`. The value must
  have exactly one item per name (`E154` for a non-list, `E155` for a count
  mismatch), and names already declared become assignments.

### eng — library calls

Libraries keep their Myanmar import statement, but there is an English
spelling of it: `use <lib>;` (and `use <lib> as <alias>;` for an alias,
`use a, b;` for several at once). Calls are then written `<lib>.<fn>(args);`
— the same call as `<lib> ၏ <fn>(args) ကို လုပ်ပါ။`.

```
use ကျပန်း;                      # = နည်းပညာများ ကျပန်း ကို အသုံးပြုပါ။

time = ကျပန်း.ကိန်း(1, 100);     # = ကျပန်း ၏ ကိန်း(1, 100) ကို လုပ်ပါ။
print(time);

n :int = ကျပန်း.ကိန်း(1, 10);   # in a typed declaration
pick = r.တန်ဖိုး(["A"]);         # through an alias: `use ကျပန်း as r;`
အချိန်.စောင့်(0);               # as a statement: the value is discarded
```

A library function that produces no value (like `အချိန်`'s `စောင့်`) may be
called as a statement, but using it as a value is `E117`.

#### Library functions as values

Leaving off the call binds the function itself: `<lib>.<fn>` is a
first-class value that can be stored and called later. When the declaration
carries a type annotation, that type is the type the call will return.

```
use Tools;                       # = နည်းပညာများ Tools ကို အသုံးပြုပါ။

TOOL :int = Tools.add;           # bind Tools.akk's `add`
r :int = TOOL(1, 100);           # 101
print(r);

ALIAS = Tools.add;               # untyped binding
print(ALIAS(2, 3));              # 5
ALIAS(4, 5);                     # as a statement: the value is discarded

var အတွက် TOOL(1, 100) ကို လုပ်ပါ။   # the call sentence spelling
var ကို ဖော်ပြပါ။                  # 101
```

A binding can also be called through the Myanmar forms
(`<name> ကို လုပ်ရန် <args> ဖြင့်။`, or the call sentence
`<name>(<args>) ကို လုပ်ပါ။`). Creating the binding checks that the library
is imported and, for a script or package, that it defines that function
(`E089`, `E085`, `E086`); calling a binding whose function returns nothing is
`E117`, like any other library call.

Both spellings share one namespace: `pin` constants are immutable everywhere,
eng values work inside Myanmar statements (and vice versa), and an eng `fn` is
an ordinary Akkhara function — Myanmar statements can call it with
`<fn> ကို လုပ်ရန် <args> ဖြင့်။`.

eng error codes (English messages, catchable with the usual try/catch form):
`E100` bad declaration form, `E101` unknown type, `E102` missing value,
`E103` declaration type mismatch, `E104` `pin` reassignment, `E105` `pin`
redeclaration, `E106` bad call form, `E107` input couldn't convert, `E108`
`return` outside a function, `E109` bad block form, `E110` bad builtin
arguments, `E111` `sqrt` of a negative number, `E113` unknown function,
`E114` function returned no value, `E115` loop used as a value ended without
one, `E116` `break` outside a loop, `E117` library call used as a value but
returned nothing, `E118` bad `use` import form.

### Known limitations
- There is no automatic terminator: every statement needs its `;` or `။`,
  even when a line break makes the end obvious.
- Default parameters are accepted on `fn`, lambda and `လုပ်ငန်း` headers only;
  a class constructor takes plain parameter names.
- Destructuring takes a list or tuple of plain names: no nested patterns and
  no dict/object destructuring.
- `switch` is a statement, not a value. Where a value is needed, use a ternary
  or an `if` that returns.
- eng builtins (`abs`, `upper`, `max`, ...) are not values, so they cannot be
  used as callbacks (`map(xs, abs)` is `E030`); library functions and lambdas
  can.
- `sort` takes a list and no comparator.
- `App` callbacks (`button`, `on_key`, `every`) take the *name* of a function,
  as text: `"on_go"`.
