# request — an HTTP library for Akkhara

Akkhara's equivalent of Python's `requests`. It is a **built-in**
library compiled into the `akk` binary (like `ကျပန်း` and `အချိန်`),
because real network I/O can't be written in plain Akkhara source.

## Import

```
နည်းပညာများ request ကို အသုံးပြုပါ။
```

## Syntax

The library uses the `၏` ("of") member particle:

```
<lib> ၏ <mod>
<var> သည် <lib> ၏ <mod> ဖြစ်၏။
```

### GET

```
response သည် request ၏ "https://api.example.com/data" ဖြစ်၏။
```

### POST

```
y အတွက် request ၏ "https://api.example.com/submit" သို့ "a=1&b=2" ကို ပို့ပါ။
```

## The response object

Both GET and POST produce a response object (`တုံ့ပြန်ချက်`). Read its
fields with the same `၏` particle:

| Field | Type | Meaning |
|---|---|---|
| `အခြေအနေကုဒ်` | ကိန်းပြည့် | HTTP status code, e.g. `200` |
| `စာသား` | စာသား | response body as text |
| `အောင်မြင်` | မှန်/မှား | `မှန်` when the status is 2xx |
| `လိပ်စာ` | စာသား | the final URL (after redirects) |

```
response ၏ အခြေအနေကုဒ် ကို ဖော်ပြပါ။
response ၏ စာသား ကို ဖော်ပြပါ။
response ၏ အောင်မြင် ကို ဖော်ပြပါ။
response ၏ လိပ်စာ ကို ဖော်ပြပါ။
```

## Full example

```
နည်းပညာများ request ကို အသုံးပြုပါ။

response သည် request ၏ "https://api.example.com/data" ဖြစ်၏။
response ၏ အခြေအနေကုဒ် ကို ဖော်ပြပါ။

y အတွက် request ၏ "https://api.example.com/submit" သို့ "a=1&b=2" ကို ပို့ပါ။
y ၏ စာသား ကို ဖော်ပြပါ။
```

## Content types

The POST body's `Content-Type` is chosen automatically:

- starts with `{` or `[` → `application/json`
- contains `=` and no spaces → `application/x-www-form-urlencoded`
- otherwise → `text/plain; charset=utf-8`

## Errors

A non-2xx response is **not** an error — you get a normal response
object so you can inspect `အခြေအနေကုဒ်` and `စာသား` yourself (an API's
JSON error message is often the useful part). Only the problems below
raise a catchable Akkhara error:

| Code | Cause |
|---|---|
| `E068` | the library was used without `နည်းပညာများ request ကို အသုံးပြုပါ။` |
| `E070` | transport failure — no internet, DNS failure, connection refused, TLS error, timeout |
| `E071` | the response body couldn't be read |
| `E072` | invalid URL — empty, missing `http://`/`https://`, or missing host |
| `E073` | response larger than the 10 MB limit |
| `E074` | nothing readable after `၏` |
| `E075` | malformed `ပို့ပါ` statement |
| `E076` | that field doesn't exist on the object (the message lists the ones that do) |
| `E077` | `၏` used on a non-object value |
| `E078` | URL or POST body wasn't text |

All of them work with `စမ်းရန် / ဖမ်းပါ`:

```
စမ်းရန်
    response သည် request ၏ "https://api.example.com/data" ဖြစ်၏။
    response ၏ စာသား ကို ဖော်ပြပါ။
E070 ကို ဖမ်းပါ။
    "အင်တာနက် ချိတ်ဆက်၍ မရပါ" ကို ဖော်ပြပါ။
ပြီး။
```

Use `E ကို ဖမ်းပါ။` to catch any error regardless of code.

## Limits

- 30-second request timeout
- 10 MB maximum response size
- non-UTF-8 responses are decoded leniently rather than failing

## Files

```
main.rs     <- Rust source, compiled into the akk binary
README.md   <- this file
```
