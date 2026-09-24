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

## Install the `akk` command (Windows / PowerShell)

See the comment block at the top of `command.ps1` for full instructions —
short version: build once with `cargo build --release`, then add this line
to your PowerShell `$PROFILE`:

```powershell
function akk { & "C:\path\to\akkhara\command.ps1" @args }
```

Restart PowerShell and `akk file.akk` works from anywhere.

## Language reference

- Variable: `<name> သည် <value> ဖြစ်၏။`
- Print: `<value> ကို ဖော်ပြပါ။`
- Input (discard): `<prompt> ကို မေးပါ။`
- Input (assign): `<name> အတွက် <prompt> ကို မေးပါ။`
- Type conversion: `<value> ကို <type> သို့ ပြောင်းပါ။` (types: `ကိန်းပြည့်`, `ဒဿမကိန်း`, `စာသား`)
- Math assignment: `<var or value> ကို <amount> <fn>ပါ။` or `<var> အတွက် <amount> <fn>ပါ။`
  (fn: `တိုး` increase, `လျော့` reduce, `မြှောက်` multiply, `စား` divide — the `အတွက်` form defaults an undeclared variable to `0` before applying the operation)
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

### Type keywords
`စာသား`(str) `ကိန်းပြည့်`(int) `ဒဿမ`/`ဒဿမကိန်း`(float) `မှန်/မှား`(bool) `စာရင်း`(list) `အစု`(tuple) `အုပ်စု`(set) `အဘိဓာန်`(dict) `ဇယား`(table)

### Libraries

Import a library, then reach its functions with the `၏` particle.

- Import: `နည်းပညာများ <lib>[, <lib>] ကို အသုံးပြုပါ။`
- Call (no arguments): `<lib> ၏ <fn> ကို လုပ်ပါ။`
- Call with arguments in parens: `<lib> ၏ <fn>(<arg>) ကို လုပ်ပါ။`
- Call with `လုပ်ရန်`/`ဖြင့်`: `<lib> ၏ <fn> ကို လုပ်ရန် <arg> ဖြင့်။`
  (the argument(s) may also be parenthesized: `<lib> ၏ <fn> ကို လုပ်ရန် (<arg>) ဖြင့်။`)
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

```
နည်းပညာများ request ကို အသုံးပြုပါ။

response အတွက် request ၏ get("https://api.example.com/data") ကို လုပ်ပါ။
response ၏ အခြေအနေကုဒ် ကို ဖော်ပြပါ။
```

All error messages are in Myanmar, formatted as `လိုင်း <N> ...`.

### Known limitations
- One statement per physical line (no multi-line statements).
- Nested expressions (e.g. a conversion used directly inside a print's value) aren't supported — use a variable as an intermediate step.
- Generic user-defined `function ... ကို လုပ်ပါ` calling isn't implemented (per spec, only noted for future reference).
