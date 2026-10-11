# json — reading and writing JSON (ဂျေဆန်)

`json` is compiled into `akk` like the other libraries: nothing to install, no
extra runtime, and **no dependencies** — not even `serde`. The grammar is
small, so the parser and the writer are in this repository
([`main.rs`](main.rs)) and the module is plain Rust with its own tree type; the
interpreter converts at the boundary (`eval_json_call` in `src/interpreter.rs`).

It exists because a backend has to speak JSON. Before it, a program could not
read a request body or write a response body that another service could parse:
the only JSON support in the tree was the Rust side's own, unreachable from an
`.akk` program.

The library has two spellings, `json` (English) and `ဂျေဆန်` (Myanmar), and every
function has a name in each; both import lines and both call forms reach the
same implementation.

```
use json;                                # English import
နည်းပညာများ ဂျေဆန် ကို သုံးမည်။            # Myanmar import
```

## Functions

| English | Myanmar | Arguments | Result |
|---|---|---|---|
| `parse` | `ဖတ်` | `(text)` | the value the text describes, or `E132` |
| `stringify` | `ရေးပါ` | `(value)` | one-line JSON text |
| `pretty` | `လှပစွာ` | `(value)` | JSON text indented two spaces per level |
| `get` | `ရယူ` | `(container, key, default)` | the member, or `default` |

`get` is the lenient member read a handler needs most: it looks a key up in a
dict, or a field up in an object (a class instance), and answers `default` when
the key is missing **or when the value has no members at all** — so
`json.get(req ၏ body, "name", "?")` cannot fail the request it is serving.
Everything else fails loudly.

## What a JSON value becomes

| JSON | language | back again |
|---|---|---|
| `null` | `null`, printed as `null` | `null` |
| `true` / `false` | `မှန်` / `မှား` | `true` / `false` |
| `1`, `-30` | `ကိန်းပြည့်` (int) | `1`, `-30` |
| `1.0`, `3.5`, `1e3` | `ဒဿမကိန်း` (float) | `1.0`, `3.5`, `1000.0` |
| `"text"` | `စာသား` (str) | a quoted, escaped string |
| `[a, b]` | `စာရင်း` (list) | `[a, b]` |
| `{...}` | `အဘိဓာန်` (dict), text keys | `{...}`, keys in their order |

A whole number with no fraction and no exponent becomes an int, so
`{"n": 1}` round-trips as `{"n": 1}` rather than `1.0`. A key written twice
keeps its first position with its last value — the same rule a dict literal
follows.

In the other direction a program's values are written out as JSON: a class
instance becomes an object of its fields (`json.stringify(some_object)` works),
a tuple or a set becomes an array, a dict key that is not text uses its printed
form, and a float that is not finite (`NaN`, infinity) becomes `null`, which is
the only thing JSON can say. A function has no JSON form at all (`E133`).

## Writing JSON text in a program

`{` inside a string starts interpolation, so a literal brace is escaped:
**`\{` and `\}` are literal braces**. That is how JSON (or CSS, or a template)
is written inside a string:

```
use json;

body = "\{\"name\": \"Aung\", \"age\": 30, \"ok\": true\}";
d = json.parse(body);
print(json.stringify(d));
print(json.get(d, "age", 0));
```

```
{"name":"Aung","age":30,"ok":true}
30
```

The same program in the Myanmar spelling — the import line and the calls
change, the handler-visible text does not:

```
နည်းပညာများ ဂျေဆန် ကို သုံးမည်။

ဒေတာ မှာ "\{\"name\": \"Aung\"\}" ဖြစ်၏။
d အတွက် ဂျေဆန် ၏ ဖတ်(ဒေတာ) ကို လုပ်ပါ။
ဂျေဆန် ၏ ရေးပါ(d) ကို ဖော်ပြပါ။
```

## With the backend

The two libraries are the backend loop: `json.parse` on the way in,
`json.stringify` (or a built string) on the way out.

```
fn create_user(req) {
    body = json.parse(req ၏ body);
    name = json.get(body, "name", "(anon)");
    reply_with = "\{\"id\": 7, \"name\": \"" + name + "\"\}";
    return web.reply(201, reply_with, "application/json");
}
```

## Error codes

| Code | Meaning |
|---|---|
| `E132` | the text is not JSON; the message names the problem and the byte offset where it was found |
| `E133` | the value cannot be written as JSON (it is a function) |
| `E086` | no such function in the library |
| `E087` | the wrong number of arguments |

Nesting is capped at 128 levels and the parse is recursive-descent over the
whole text at once, so a body of `[[[[[...` is refused with `E132` instead of
overflowing the stack.

## Deliberately not covered

* **Streaming** — `parse` wants the whole text; there is no incremental reader
  or writer, and no JSON Lines helper.
* **Mutation** — a parsed body is an ordinary dict/list, and the language has no
  element assignment yet, so a parsed value is read-only: build a new dict (or
  a response string) instead of editing one.
* **Ordering guarantees beyond document order, JSON Pointer, JSON Patch,
  `NaN`/`Infinity` literals, 64-bit-exact big numbers** — numbers are `i64` or
  `f64`, and nothing beyond plain RFC 8259 text is understood.

## Checking it

`benchmarks/json/english.akk` and `benchmarks/json/myanmar.akk` are the same
program in the two spellings; they must print byte-identical output, and
`benchmarks/diffcheck.py` runs both through two builds and compares what came
out. The backend probes under `benchmarks/server/` use `parse`/`get` on a real
request body, driven from outside the interpreter by
`python benchmarks/servercheck.py --akk ./target/release/akk.exe`.
