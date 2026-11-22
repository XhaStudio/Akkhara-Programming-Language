# Akkhara — all keywords and functions (Myanmar ↔ English)

Every entry below is taken from the implementation: the keyword constants in
`src/frontend/parser/`, the peelable-suffix list in `src/frontend/lexer/`, the
dispatch tables in `src/interp/`, and the built-in library modules under
`src/stdlib/`.

A dash (`—`) in the English column means the feature has no English spelling:
it is Myanmar-only syntax. English builtins (listed in the last section) are
English-only.

---

## 1. Statement and particle keywords

| Myanmar | English / meaning                  | Used in                                   |
| ------- | ---------------------------------- | ----------------------------------------- |
| `သည်`   | `is` (declaration marker)          | `<name> သည် <value> ဖြစ်၏။`               |
| `မှာ`   | `in` (collection declaration)      | `<name> မှာ [1, 2] ဖြစ်၏။`                |
| `ဖြစ်၏` | `equals` (closes a declaration)    | `<name> သည် <value> ဖြစ်၏။`               |
| `က`     | short declaration                  | `<name> က <value>။`                       |
| `ကို`   | object particle (`to`, accusative) | `<value> ကို ဖော်ပြပါ။`                   |
| `၏`     | `of` (member / library access)     | `<lib> ၏ <fn>`, `obj ၏ field`             |
| `ဖြင့်` | `with` (argument list)             | `<fn> ကို လုပ်ရန် <arg> ဖြင့်။`           |
| `သို့`  | `to` (request send / `or`)         | `request ၏ <url> သို့ <data> ကို ပို့ပါ။` |
| `အဖြစ်` | `as` (alias)                       | `<fn> အဖြစ် <alias>။`                     |
| `အတွက်` | `for` (loop / assign-to)           | `x အတွက် တစ်ခုစီ(...)`                    |

## 2. Input / output keywords

| Myanmar    | English / meaning | Used in                 |
| ---------- | ----------------- | ----------------------- |
| `ဖော်ပြပါ` | `print`           | `<value> ကို ဖော်ပြပါ။` |
| `ပြပါ`     | `print` (short)   | `<value> ကို ပြပါ။`     |
| `မေးပါ`    | `input` / `ask`   | `<prompt> ကို မေးပါ။`   |

## 3. Function and class keywords

| Myanmar    | English / meaning                | Used in                          |
| ---------- | -------------------------------- | -------------------------------- |
| `အလုပ်`    | `fn` (function definition)       | `အလုပ် <name> သည် ... ပြီး။`     |
| `လုပ်ငန်း` | `fn` (older spelling)            | `လုပ်ငန်း <name> သည် ... ပြီး။`  |
| `လုပ်ပါ`   | `do` / call                      | `<fn> ကို လုပ်ပါ။`               |
| `လုပ်ရန်`  | `to do` / call-with              | `<fn> ကို လုပ်ရန် <arg> ဖြင့်။`  |
| `ပြီး`     | `end` (closes a block)           | `... ပြီး။`                      |
| `ပုံသေ`    | `class`                          | `ပုံသေ <name> သည် ... ပြီး။`     |
| `အတု`      | `self`                           | `အတု <field> သည် <value> ဖြစ်၏။` |
| `အသစ်`     | `new` (removed marker, reported) | —                                |

## 4. Control-flow keywords

| Myanmar                                    | English / meaning                  | Used in                                    |
| ------------------------------------------ | ---------------------------------- | ------------------------------------------ |
| `အကယ်၍`                                    | `if`                               | `အကယ်၍ (<cond>) ဖြစ်လျှင်`                 |
| `ဖြစ်လျှင်`                                | `then` (positive)                  | `(<cond>) ဖြစ်လျှင်`                       |
| `ဖြစ်ရင်`                                  | `then` (short)                     | `(<cond>) ဖြစ်ရင်`                         |
| `မဖြစ်လျှင်`                               | `then not` (negative)              | `(<cond>) မဖြစ်လျှင်`                      |
| `မဖြစ်ရင်`                                 | `then not` (short)                 | `(<cond>) မဖြစ်ရင်`                        |
| `သို့မဟုတ်`                                | `elif` (else-if)                   | `သို့မဟုတ် (<cond>) ဖြစ်လျှင်`             |
| `အခြား`                                    | `elif` (short)                     | `အခြား (<cond>) ဖြစ်ရင်`                   |
| `မဟုတ်လျှင်`                               | `else`                             | `မဟုတ်လျှင်`                               |
| `မဟုတ်ရင်`                                 | `else` (short)                     | `မဟုတ်ရင်`                                 |
| `အခြေအနေ`                                  | `while`                            | `အခြေအနေ (<cond>) ဖြစ်နေစဉ်`               |
| `ဖြစ်နေစဉ်` / `ဖြစ်နေစဥ်` / `ဖြစ်နေရင်`    | `while` (positive)                 | `(<cond>) ဖြစ်နေစဉ်`                       |
| `မဖြစ်နေစဉ်` / `မဖြစ်နေစဥ်` / `မဖြစ်နေရင်` | `while not` (negative)             | `(<cond>) မဖြစ်နေစဉ်`                      |
| `ထဲ`                                       | `in` (short loop particle)         | `x သည် xs ထဲ တစ်ခုစီ`                      |
| `ထဲမှ`                                     | `from inside` (long loop particle) | `x သည် xs ထဲမှ တစ်ခုစီ`                    |
| `တစ်ခုစီ`                                  | `each` (`for`)                     | `x သည် တစ်ခုစီ(0, 10)`                     |
| `ရပ်ပါ`                                    | `break`                            | `<value> ကို ရပ်ပါ။`                       |
| `ဆက်ပါ`                                   | `continue`                         | `ဆက်ပါ။`                                   |
| `ပစ်ပါ`                                   | `throw` (raise a user error)       | `<value> ကို <name> အမည်ဖြင့် ပစ်ပါ။`     |
| `အမည်ဖြင့်`                               | `with the name` (error identity)   | `<value> ကို <name> အမည်ဖြင့် ပစ်ပါ။`     |
| `ရွေးရန်`                                  | `switch`                           | `ရွေးရန် (<value>) ... ပြီး။`              |
| `မူလ`                                      | `default`                          | `မူလ ...`                                  |
| `စမ်းရန်`                                  | `try`                              | `စမ်းရန် ... ဖမ်းပါ ... နောက်ဆုံးတွင် ...` |
| `ဖမ်းပါ`                                   | `catch`                            | `ဖမ်းပါ`                                   |
| `နောက်ဆုံးတွင်`                            | `finally`                          | `နောက်ဆုံးတွင်`                            |
| `နှင့်`                                    | `and`                              | `(<a>) နှင့် (<b>)`                        |
| `သို့`                                     | `or`                               | `(<a>) (သို့) (<b>)`                       |
| `and` / `or`                               | English `and` / `or`               | `if (a > 0 && b > 0)`                      |
| `throw`                                    | English `throw` (raise)            | `throw <value> as <name>;`                 |

### Raising and catching on the native backend (`akk build <file>`)

The raise and catch spellings above are all part of the native backend's slice,
with these limits:

- `ပစ်ပါ` / `throw` needs its value to be text (`str`): a catch clause binds
  that text and an uncaught raise prints it. Any other value is refused with
  `E177` (the interpreter accepts every value).
- `<name> ကို ဖမ်းပါ။` (the raise's identity), `E178 ကို ဖမ်းပါ။` (every
  raise) and `E ကို ဖမ်းပါ။` (anything) all work, including a raise made inside
  a called function that unwinds to the caller's clause.
- A clause naming any other `E###` code is refused with `E177`: a compiled
  program has no catchable runtime errors, so such a clause could never run.
- A `break`, `continue` or `return` that would leave a `finally` without
  running it is refused with `E177`.
- An uncaught raise writes `E178 လိုင်း <line> တွင် [<name>] <message>` to
  stderr and exits 1, exactly as the interpreter and the VM do.

## 5. Arithmetic / assignment keywords

| Myanmar                 | English / meaning                  | Used in                           |
| ----------------------- | ---------------------------------- | --------------------------------- |
| `တိုး` / `တိုးပါ`       | `increase` (`+=`)                  | `<var> ကို 1 တိုးပါ။`             |
| `လျော့` / `လျော့ပါ`     | `reduce` (`-=`)                    | `<var> ကို 1 လျော့ပါ။`            |
| `မြှောက်` / `မြှောက်ပါ` | `multiply` (`*=`)                  | `<var> ကို 2 မြှောက်ပါ။`          |
| `စား` / `စားပါ`         | `divide` (`/=`)                    | `<var> ကို 2 စားပါ။`              |
| `တိုးခြင်းဖြင့်`        | loop step `+`                      | `... တစ်ခုစီ 2 တိုးခြင်းဖြင့်`    |
| `လျော့ခြင်းဖြင့်`       | loop step `-`                      | `... တစ်ခုစီ 1 လျော့ခြင်းဖြင့်`   |
| `မြှောက်ခြင်းဖြင့်`     | loop step `*`                      | `... တစ်ခုစီ 2 မြှောက်ခြင်းဖြင့်` |
| `စားခြင်းဖြင့်`         | loop step `/`                      | `... တစ်ခုစီ 2 စားခြင်းဖြင့်`     |
| `ပြောင်းပါ`             | `convert` (removed verb, reported) | —                                 |

Operator names used in error messages: `ပေါင်း` (+), `နှတ်` (-), `မြှောက်` (\*),
`စား` (/), `ကြွင်းကိန်းရှာ` (%), `ထပ်ကိန်းတင်` (`**`), `အပြည့်ကိန်းစား` (`//`).

## 6. Library keywords

| Myanmar                     | English / meaning                          | Used in                                     |
| --------------------------- | ------------------------------------------ | ------------------------------------------- |
| `နည်းပညာများ`               | `libraries` (import header)                | `နည်းပညာများ <lib> ကို အသုံးပြုပါ။`         |
| `သုံးမည်`                   | `use` (import verb)                        | `... ကို သုံးမည်။`                          |
| `အသုံးပြုပါ`                | `use` (import verb, README spelling)       | `... ကို အသုံးပြုပါ။`                       |
| `စောင့်ပါ`                  | `wait` (sleep statement)                   | `10s ကို စောင့်ပါ။`                         |
| `ကျပန်းကိန်း`               | `random_int` (statement)                   | `ကျပန်းကိန်း x သည် 1 နှင့် 10 အကြား ဖြစ်၏။` |
| `အကြား`                     | `between`                                  | `1 နှင့် 10 အကြား`                          |
| `ပို့ပါ`                    | `send` (`request` POST sentence)           | `request ၏ <url> သို့ <data> ကို ပို့ပါ။`   |
| `ဖိုင်` / `ဂျေဆန်` / `ဆာဗာ` | library aliases `file` / `json` / `server` | `နည်းပညာများ ဖိုင် ကို အသုံးပြုပါ။`         |

Language keywords the lexer also peels off when fused to a word:
`ပြောင်းပါ`, `ဖော်ပြပါ`, `မြှောက်ပါ`, `လျော့ပါ`, `စားပါ`, `တိုးပါ`, `မေးပါ`,
`အတွက်`, `ဖြစ်၏`, `သည်`, `မှာ`, `ကို`, `သို့`.

## 7. Type keywords and literals

| Myanmar            | English type            | Notes                    |
| ------------------ | ----------------------- | ------------------------ |
| `စာသား`            | `str`                   | text                     |
| `ကိန်းပြည့်`       | `int`                   | integer                  |
| `ဒဿမ` / `ဒဿမကိန်း` | `float`                 | decimal                  |
| `မှန်` / `မှား`    | `True` / `False` `bool` | booleans                 |
| `စာရင်း`           | `list`                  | `[1, 2, 3]`              |
| `အစု`              | `tuple`                 | `(1, 2, 3)`              |
| `အုပ်စု`           | `set`                   | `{1, 2, 3}`              |
| `အဘိဓာန်`          | `dict`                  | `{ "k" သည် v ဖြစ်၏။ }` / `{ "k" က v။ }` |
| `ဇယား`             | `table`                 | list of lists            |
| `ကိန်း`            | `number`                | broad numeric type-check |
| `null` / `ဗလာ`     | `null`                  | null literal             |

## 8. Conversion functions (core syntax)

| Myanmar                  | English     | Result       |
| ------------------------ | ----------- | ------------ |
| `စာ(x)`                  | `str(x)`    | string       |
| `ကိန်းပြည့်(x)`          | `int(x)`    | integer      |
| `ဒဿမ(x)` / `ဒဿမကိန်း(x)` | `float(x)`  | float        |
| `ဘူးလ်(x)` / `ဘူလ်(x)`   | `bool(x)`   | boolean      |
| `ကိန်း(x)`               | `digits(x)` | numeric text |

## 9. Builtin functions (English names only — usable in any statement)

| Function                                                  | Arguments                | Result             |
| --------------------------------------------------------- | ------------------------ | ------------------ |
| `len(x)`                                                  | string or collection     | length             |
| `abs(x)`                                                  | number                   | absolute value     |
| `min(...)` / `max(...)`                                   | values or one collection | smallest / largest |
| `sqrt(x)`                                                 | number ≥ 0               | square root        |
| `floor(x)` / `ceil(x)` / `round(x)`                       | number                   | integer            |
| `upper(s)` / `lower(s)` / `trim(s)`                       | string                   | new string         |
| `contains(hay, needle)`                                   | string/collection, value | boolean            |
| `split(s, sep)`                                           | two strings              | list               |
| `join(list, sep)`                                         | collection, string       | string             |
| `replace(s, from, to)`                                    | three strings            | string             |
| `find(s, needle)` / `index_of`                            | two strings              | index              |
| `starts_with(s, part)` / `ends_with(s, part)`             | two strings              | boolean            |
| `keys(d)` / `values(d)` / `entries(d)`                    | dict                     | list               |
| `enumerate(xs)`                                           | collection               | list of pairs      |
| `slice(xs, start, end)`                                   | collection, numbers      | new collection     |
| `sum(xs)`                                                 | collection of numbers    | number             |
| `map(xs, fn)` / `filter(xs, fn)` / `reduce(xs, fn, init)` | collection, callback     | collection / value |
| `push(xs, v)` / `append(xs, v)`                           | list, value              | mutates list       |
| `pop(xs)`                                                 | list                     | last item          |
| `insert(xs, i, v)` / `remove(xs, i)`                      | list, index              | mutates list       |
| `sort(xs)`                                                | list                     | mutates list       |
| `reverse(xs)`                                             | collection or string     | reversed copy      |

## 10. Library functions

### `request` (English name only)

| Function          | Arguments      | Result          |
| ----------------- | -------------- | --------------- |
| `get(url)`        | url            | response object |
| `post(url, data)` | url, body text | response object |

Response fields: `အခြေအနေကုဒ်` (status code), `အကြောင်းအရာ` (body),
`အောင်မြင်မှု` (success boolean).

### `ကျပန်း` — random

| Myanmar               | English        | Arguments      | Result             |
| --------------------- | -------------- | -------------- | ------------------ |
| `ကိန်း` / `ကိန်ပြည့်` | `random_int`   | `(min, max)`   | integer            |
| `ဒဿမ`                 | `random_float` | `(min, max)`   | float              |
| `တန်ဖိုး`             | `value`        | `(collection)` | one random element |

### `အချိန်` — time

| Myanmar  | English | Arguments   | Result              |
| -------- | ------- | ----------- | ------------------- |
| `စောင့်` | `wait`  | `(seconds)` | none (blocks)       |
| `ယခု`    | `now`   | `()`        | Unix time (integer) |

### `json` / `ဂျေဆန်`

| English     | Myanmar  | Arguments                   | Result             |
| ----------- | -------- | --------------------------- | ------------------ |
| `parse`     | `ဖတ်`    | `(text)`                    | parsed value       |
| `stringify` | `ရေးပါ`  | `(value)`                   | one-line JSON text |
| `pretty`    | `လှပစွာ` | `(value)`                   | indented JSON text |
| `get`       | `ရယူ`    | `(container, key, default)` | member or default  |

### `file` / `ဖိုင်`

| English  | Myanmar      | Arguments      | Result           |
| -------- | ------------ | -------------- | ---------------- |
| `read`   | `ဖတ်`        | `(path)`       | file text        |
| `lines`  | `လိုင်းများ` | `(path)`       | list of lines    |
| `write`  | `ရေး`        | `(path, text)` | none (truncates) |
| `append` | `ထပ်`        | `(path, text)` | none (creates)   |
| `exists` | `ရှိ`        | `(path)`       | boolean          |
| `list`   | `စာရင်း`     | `(folder)`     | sorted names     |
| `size`   | `အရွယ်`      | `(path)`       | bytes            |
| `delete` | `ဖျက်`       | `(path)`       | none             |

### `server` / `ဆာဗာ`

| English    | Myanmar         | Arguments                        | Result               |
| ---------- | --------------- | -------------------------------- | -------------------- |
| `get`      | `ရယူ`           | `(path, handler)`                | registers GET        |
| `post`     | `ပို့`          | `(path, handler)`                | registers POST       |
| `put`      | `အစားထိုး`      | `(path, handler)`                | registers PUT        |
| `delete`   | `ဖျက်`          | `(path, handler)`                | registers DELETE     |
| `patch`    | `ပြင်ဆင်`       | `(path, handler)`                | registers PATCH      |
| `route`    | `လမ်းကြောင်း`   | `(method, path, handler)`        | registers any method |
| `reply`    | `အဖြေ`          | `(status, body[, content_type])` | response             |
| `header`   | `ခေါင်းစီးထည့်` | `(response, name, value)`        | response             |
| `redirect` | `ပြန်ညွှန်း`    | `(url[, status])`                | response             |
| `serve`    | `ဆောင်ရွက်`     | `(port)`                         | none (blocks)        |

Request fields: `method`/`နည်းလမ်း`, `path`/`လမ်း`, `query`/`မေးခွန်း`,
`query_params`/`မေးခွန်းအပိုင်း`, `path_params`/`လမ်းအပိုင်း`,
`body`/`ကိုယ်ထည်`, `headers`/`ခေါင်းစီး`.

### `App` (English function names only)

| Group            | Functions                                                                                                                                                      |
| ---------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| window           | `screen(width, height)`, `title(w, text)`, `run(w)`, `close(w)`                                                                                                |
| widgets          | `label`, `button`, `input`, `textarea`, `checkbox`, `choice`, `listbox`, `table`, `image`, `canvas`                                                            |
| values           | `get(widget)`, `set(widget, value)`, `number(widget)`, `items(widget)`, `set_items(widget, list)`, `cell(table, row, col)`, `set_cell(table, row, col, value)` |
| geometry & style | `move`, `size`, `color`, `font`, `show`, `hide`, `enable`, `disable`                                                                                           |
| events           | `on_key(w, "Enter", "fn")`, `every(w, milliseconds, "fn")`                                                                                                     |
| dialogs          | `message(text)`, `ask(text)`, `pick_file()`                                                                                                                    |
| canvas           | `rect`, `circle`, `line`, `text`, `clear`                                                                                                                      |
