# server — the Akkhara backend library (ဆာဗာ)

`server` is compiled into `akk` like the other libraries: no package to install
and no extra runtime. A program imports it, registers one handler per method +
path, and calls `serve(port)` to answer requests. Everything is
[`std::net`](https://doc.rust-lang.org/std/net/) plus one connection at a time —
no async runtime, no TLS stack, no new dependencies.

The surface is the one a small backend actually needs: route patterns with
`:name` placeholders, the query parsed into a dict, `GET`/`POST`/`PUT`/`PATCH`/
`DELETE` (and `HEAD`, answered from the `GET` route), a choice of status, body
and `Content-Type`, response headers a handler sets itself, a redirect, a `405`
with an `Allow` header when the path exists but the method does not, and
percent-decoding of path parameters and query text. Bodies are read and written
as text, so JSON goes through the [`json`](../json/README.md) library.

The library has two spellings, `server` (English) and `ဆာဗာ` (Myanmar), and every
function has a name in each; both import lines and both call forms reach the
same implementation.

```
use server;                              # English import
နည်းပညာများ ဆာဗာ ကို သုံးမည်။              # Myanmar import
```

> The Myanmar import line the parser accepts is `… ကို သုံးမည်။`: the verb is
> `သုံးမည်`, and the `ကို` particle before it is required (leaving it out is
> `E094`).

## Functions

| English | Myanmar | Arguments | Result |
|---|---|---|---|
| `get` | `ရယူ` | `(path, handler)` | none — registers a GET route |
| `post` | `ပို့` | `(path, handler)` | none — registers a POST route |
| `put` | `အစားထိုး` | `(path, handler)` | none — registers a PUT route |
| `delete` | `ဖျက်` | `(path, handler)` | none — registers a DELETE route |
| `patch` | `ပြင်ဆင်` | `(path, handler)` | none — registers a PATCH route |
| `route` | `လမ်းကြောင်း` | `(method, path, handler)` | none — registers any method |
| `reply` | `အဖြေ` | `(status, body[, content_type])` | response value, for a handler to return |
| `header` | `ခေါင်းစီးထည့်` | `(response, name, value)` | the same response with one more header |
| `redirect` | `ပြန်ညွှန်း` | `(url[, status])` | a response with `Location` and no body |
| `serve` | `ဆောင်ရွက်` | `(port)` | none — blocks, answering requests |

`path`, `method`, `handler` and `url` are text. `handler` is the **name** of an
Akkhara function, written as text — the same convention `App` uses for button
callbacks (`web.get("/hello", "hello")` calls `fn hello(req)`). A `HEAD`
request is answered from the `GET` route for the same path: same status and
headers, body left off, `Content-Length` still describing the body it would
have sent. There is no `head(...)` registration of its own.

### Route patterns

A segment that starts with `:` captures whatever is in that position:

```
web.get("/users/:id", "show_user");      # GET /users/42  -> id = "42"
web.get("/users/:id/posts/:slug", "show_post");
```

A captured value is percent-decoded (`/users/a%20b` gives `a b`) and arrives in
the request's `path_params`. The rest of the path must match exactly, segment
by segment: there is no `*`, no optional segment and no regular expression.
`/users/:id` and `/users/:name` are the same route (registering both is `E130`),
while `/users/me` is a different one. When both match a request, the literal
route wins.

## The request

The handler is called with one argument: the request. Its fields are readable
with the usual `<expr> ၏ <field>` syntax, and each field answers to both
spellings:

| Field | Holds |
|---|---|
| `method` / `နည်းလမ်း` | `"GET"`, `"POST"`, … |
| `path` / `လမ်း` | the requested path, as sent — e.g. `"/users/a%20b"` |
| `query` / `မေးခွန်း` | the raw text after `?`, or `""` |
| `query_params` / `မေးခွန်းအပိုင်း` | the query split into a dict, decoded |
| `path_params` / `လမ်းအပိုင်း` | what the route's `:name` placeholders captured, decoded |
| `body` / `ကိုယ်ထည်` | the request body as text |
| `headers` / `ခေါင်းစီး` | the header block as received, one `Name: value` per line |

`?a=1&b=2` is `{"a": "1", "b": "2"}` in `query_params`: `+` is a space,
`%XX` is the byte it names, a piece with no `=` has an empty value, and a
repeated key keeps its last value. Values are text — convert with the
conversion functions (`ကိန်းပြည့်(x)` / `int(x)`) when a number is wanted.
Reading a key that was not sent is a lookup miss, so use `json.get`, or check
`query_params` for the key, rather than indexing it directly.

## The response

Whatever the handler returns becomes the response:

* `reply(status, body)` (`အဖြေ`) — that status and body;
* `reply(status, body, content_type)` — the same, with its own `Content-Type`
  (`"application/json"`, `"text/html; charset=utf-8"`, …);
* a text — status 200 with that body;
* nothing — status 200 with an empty body;
* any other value — its printed form, at status 200.

A status has to be in 100–599, and the content type is used as written. Bodies
default to `text/plain; charset=utf-8`. Response headers are set by passing the
response through `header(response, name, value)` — as many times as wanted — and
`redirect(url)` is a `302` with a `Location` header and no body (`redirect(url,
301)` and the other `3xx` codes are accepted). Header values with a CR or LF in
them are stripped before they reach the wire, so a header built from request
text cannot split the response.

What the library answers on its own:

* a path no route matches — `404`, `no route: <METHOD> <path>`;
* a known path, wrong method — `405`, `method not allowed: …`, with an `Allow`
  header listing what would have worked (`GET, HEAD` for a GET-only path);
* a handler that fails — `500` with its own error text as the body, and the
  server **keeps serving**;
* a request the library could not read — `400` (or `413` when `Content-Length`
is above 1 MiB), logged as `bad request -> <status>`.

Every request logs one line on stdout, so there is a trail to read (and to
redirect); a serving program never ends on its own, so it never writes the
`.akop` file that a finished run leaves:

```
server listening on port 8080
GET /users/42 -> 200
PUT /doc -> 200
POST /echo -> 500
handler error: E036 လိုင်း 12 တွင် သုညဖြင့် စား၍မရပါ။
HEAD /hello -> 200
GET /doc -> 405
GET /nope -> 404
```

## Examples

Myanmar: import and calls in the Myanmar spelling, with the handler bodies in
the eng forms the language provides for them.

```
နည်းပညာများ ဆာဗာ ကို သုံးမည်။

fn greeting(req) {
    လမ်း မှာ req ၏ လမ်း ဖြစ်၏။
    return "မင်္ဂလာပါ " + လမ်း;
}

fn form(req) {
    ကိုယ်ထည် မှာ req ၏ ကိုယ်ထည် ဖြစ်၏။
    return "လက်ခံရရှိ: " + ကိုယ်ထည်;
}

# /users/:id -- path parameter ကို လမ်းအပိုင်း ထဲမှ ဖတ်သည်
fn show_user(req) {
    အိုင်ဒီ မှာ req ၏ လမ်းအပိုင်း["id"] ဖြစ်၏။
    return "အသုံးပြုသူ " + အိုင်ဒီ;
}

# response header နှင့် redirect
fn meta(req) {
    အဖြေ အတွက် ဆာဗာ ၏ အဖြေ(200, "meta") ကို လုပ်ပါ။
    ရလဒ် အတွက် ဆာဗာ ၏ ခေါင်းစီးထည့်(အဖြေ, "X-Probe", "yes") ကို လုပ်ပါ။
    return ရလဒ်;
}

fn moved(req) {
    အဖြေ အတွက် ဆာဗာ ၏ ပြန်ညွှန်း("/hello") ကို လုပ်ပါ။
    return အဖြေ;
}

ဆာဗာ ၏ ရယူ("/hello", "greeting") ကို လုပ်ပါ။
ဆာဗာ ၏ ပို့("/form", "form") ကို လုပ်ပါ။
ဆာဗာ ၏ ရယူ("/users/:id", "show_user") ကို လုပ်ပါ။
ဆာဗာ ၏ အစားထိုး("/doc", "replace_doc") ကို လုပ်ပါ။
ဆာဗာ ၏ ရယူ("/meta", "meta") ကို လုပ်ပါ။
ဆာဗာ ၏ ရယူ("/old", "moved") ကို လုပ်ပါ။
ဆာဗာ ၏ ဆောင်ရွက်(8080) ကို လုပ်ပါ။
```

English — `use <lib> [as <alias>];`, then `<lib>.<fn>(...)`:

```
use server as web;
use json;

fn hello(req) {
    return web.reply(200, "hello " + req ၏ path);
}

fn echo(req) {
    return "echo: " + req ၏ body;
}

fn boom(req) {
    return 1 / 0;
}

# the backend loop: a JSON body in, a JSON body out
fn create_user(req) {
    body = json.parse(req ၏ body);
    name = json.get(body, "name", "(anon)");
    reply_with = "\{\"id\": 7, \"name\": \"" + name + "\"\}";
    return web.reply(201, reply_with, "application/json");
}

web.get("/hello", "hello");
web.post("/echo", "echo");
web.route("GET", "/q", "hello");
web.post("/users", "create_user");
web.get("/users/:id", "hello");
web.get("/boom", "boom");
web.serve(8080);
```

A request from anywhere else then gets an answer:

```
$ curl -i localhost:8080/hello
HTTP/1.1 200 OK
Content-Type: text/plain; charset=utf-8
Content-Length: 12
Connection: close

hello /hello
```

## Error codes

| Code | Meaning |
|---|---|
| `E129` | a `server` function got an argument of the wrong kind (paths, methods, handler names and content types are text; a status is 100–599; a redirect status is 300–399; a port is 1–65535; the first argument of `header` must be a `reply` value) |
| `E130` | a route is malformed (`/`-less path, non-uppercase method, an empty `:name`), names a handler function that is not defined, or is registered twice (two registrations that match the same requests are the same route) |
| `E131` | the port could not be listened on (usually: something else already uses it) |

A handler name is checked when the route is registered, so a typo is a startup
error rather than a 500 on the first matching request.

## Deliberately not covered

Kept out to keep the surface small, not because they are hard:

* **Concurrency** — one connection at a time, answered by the thread that
  accepted it (which is the only thread that may run Akkhara code). A slow
  request holds up the next one.
* **Keep-alive, chunked bodies, HTTP/2, TLS** — every response says
  `Connection: close`; plain HTTP only, so there is no `https://` endpoint.
* **Literal path segments are matched as sent** — only `:name` values are
  percent-decoded. A route written with a space or a non-ASCII literal segment
  will not match the percent-encoded form a browser sends; keep literals to
  plain ASCII and put anything else in a `:param`.
* **Wildcards, optional segments, regex routes, per-route middleware** — a
  pattern is literal segments plus whole-segment `:name` captures, and every
  registered route is tried in registration order (literal match first).
* **Static files, cookies, multipart uploads** — a response body is text, and a
  request body is read as text (invalid UTF-8 is replaced, not rejected), so a
  binary upload is not a thing to build here yet.
* **A request object is a plain value** — the body is not parsed for you, so
  JSON goes through the [`json`](../json/README.md) library rather than
  arriving as a dict.
* **Stopping** — `serve` never returns and there is no `stop`; a server runs
  until the process ends. Bind the port in the same program that registers the
  routes (no `bind`/`accept` split).

## Checking it

`benchmarks/servercheck.py` starts the two probes in
[`benchmarks/server/`](../../benchmarks/server/) — one per spelling — and drives
them with a real HTTP client from outside the interpreter:

```
python benchmarks/servercheck.py --akk ./target/release/akk.exe
```

It makes 38 requests: every function above, in both spellings. Each request is
checked on status, body, `Content-Type`, the handler's own header, the `Allow`
of a `405`, the `Location` of a redirect and the `Content-Length` a `HEAD`
answer must still announce — plus the server's own log lines, read back from its
stdout. The two probes carry the same set of request *shapes*, and the harness
requires them to come back with the same status, the same `Content-Type` and the
same header names, so the two spellings cannot drift apart unnoticed.
