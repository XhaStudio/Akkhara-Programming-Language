# Akkhara (အက္ခရာ)

A programming language with Myanmar-language syntax, keywords, and error messages, interpreted by a Rust binary called `akk`.

## Build

```
cargo build --release
```

The binary is produced at `target/release/akk` (`akk.exe` on Windows).

## Run

```
akk myprogram.akk
```

or, before installing the command globally:

```
./target/release/akk myprogram.akk
```

A successful run reports how long the program took on its last stdout line,
e.g. `Interpreted in 0.003s` (nothing is printed when the program fails).

Every run also saves its terminal transcript next to the source file as
`<file_name>.akop` — running `hello.akk` writes `hello.akop` holding what the
program printed followed by the run's closing line: `Interpreted in 0.003s`
when it succeeds, the error message when it fails (a program that fails
before printing anything leaves just the error). The file is written on every
run, success or failure, and each run overwrites it.

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
- Type conversion: `<value> ကို <type> သို့ ပြောင်းပါ။` (types: `ကိန်းပြည့်`, `ဒဿမကိန်း`, `စာသား`)
- Math assignment: `<var or value> ကို <amount> <fn>ပါ။` or `<var> အတွက် <amount> <fn>ပါ။`
  (fn: `တိုး` increase, `လျော့` reduce, `မြှောက်` multiply, `စား` divide — the `အတွက်` form defaults an undeclared variable to `0` before applying the operation)
- Function call: `<fn> ကို လုပ်ပါ။` (no arguments),
  `<fn> ကို လုပ်ရန် <arg>[, <arg>] ဖြင့်။`, or with the verb at the end:
  `<fn> ကို (<arg>[, <arg>]) ဖြင့် လုပ်ပါ။` — the same two spellings work for
  library functions as `<lib> ၏ <fn> ...`
- Function definition: `လုပ်ငန်း <name> သည်` ... `ပြီး။` (no parameters),
  `လုပ်ငန်း <name> အတွက် <p1>, <p2> ဖြင့်` ... `ပြီး။`, or a parenthesized header:
  `လုပ်ငန်း <name>() သည်` ... `ပြီး။` / `လုပ်ငန်း <name>(<p1>, <p2>) ဖြင့်` ... `ပြီး။`.
- Function call, bare spelling: `<fn>()` / `<fn>(<arg>[, <arg>])` -- the same call
  as `<fn> ကို လုပ်ပါ။`; a trailing `;` or `။` is optional, and one of them is
  needed when another statement follows (`greet();` or `greet()။`).
- Collections: `<name> မှာ <literal> ဖြစ်၏။`
  - List: `[1, 2, 3]`
  - Tuple: `(1, 2, 3)`
  - Set: `{1, 2, 3}` (duplicates are dropped)
  - Dict: `{ "key" သည် value ဖြစ်၏။ ... }` (each entry is its own `key သည် value ဖြစ်၏။`, can span multiple lines)
  - Table: just a list of lists, e.g. `[["a","b"], ["c","d"]]`
- Comments: `# ...`
- Arithmetic: `+ - * /` (Myanmar names in errors: ပေါင်း၊ နှတ်၊ မြှောက်၊ စား), unary minus supported (`-1`, `value * -1`, `value ကို -1 မြှောက်ပါ။`)
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
- While: `အခြေအနေ (<condition>) ဖြစ်နေစဉ်` ... `ပြီး။`, or
  `(<condition>) ဖြစ်နေစဥ်` ... `ပြီး။` -- the keyword is optional, and
  `မဖြစ်နေစဉ်` / `မဖြစ်နေစဥ်` loops while the condition is not true.
- For, over a range: `i သည် 0, 10 ထဲမှ တစ်ခုစီ 1 တိုးခြင်းဖြင့်` ... `ပြီး။`
  (the end is exclusive; the step clause is `1 တိုးခြင်းဖြင့်` and may be any number).
- For, over a collection: `x သည် xs ထဲမှ တစ်ခုစီ` ... `ပြီး။`.

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
[`libraries/App/README.md`](libraries/App/README.md).

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

[`examples/app_expenses.akk`](examples/app_expenses.akk) leans on all of
this: its eight-slot ledger lives in
[`examples/ExpensesStore.akk`](examples/ExpensesStore.akk), which the
window imports under the shorter name.

All error messages are in Myanmar, formatted as `လိုင်း <N> ...` (the `eng`
syntax below reports its own errors in English instead).

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
e.g. `if (flag && !done) { ... }`.

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
- One statement per physical line (no multi-line statements).
- Nested expressions (e.g. a conversion used directly inside a print's value) aren't supported — use a variable as an intermediate step.
- Generic user-defined `function ... ကို လုပ်ပါ` calling isn't implemented (per spec, only noted for future reference).
