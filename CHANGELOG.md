# Changelog

## 3.1.4-alpha.3

### Added

- `trim(text)` in compiled programs: the text without the Unicode whitespace at
  either end, which is the interpreter's `str::trim`. The runtime walks the
  UTF-8 forwards and backwards one scalar at a time through `rt_trim_str`, and
  `rt_ws_len` recognises the scalars `char::is_whitespace` names -- generated
  at build time from the host's own table, so the set is the interpreter's
  exactly (an ASCII space or tab, a no-break space, an ideographic space, and
  the rest) with no Unicode data of its own. A multi-byte scalar is only ever
  dropped whole, so a Myanmar syllable is never cut; the bytes between the two
  ends are copied unchanged into a fresh arena string, and a text with nothing
  to trim answers itself. A wrong arity and a non-text argument keep the
  interpreter's `E110`. `tests/native/fixtures/trim.akk` covers both optimiser
  modes (spaces, a tab and a newline, an empty text, a text of spaces, a
  binding, a parameter, a list element, a no-break space and an ideographic
  space), and `trim_arity`/`trim_of_int` pin the `E110` refusals.

### Changed

- Version bumped to `3.1.4-alpha.3`.
- The `E177` summary now names `trim`.

## 3.1.3-alpha.3

### Added

- `sqrt(x)` in compiled programs: the square root as a float, whatever the
  argument is. An int is converted to a double first, the way the interpreter's
  `as_f64` does, so `sqrt(16)` prints `4.0` and `sqrt(6.25)` is `2.5`. The
  routine is one `sqrtsd`, the instruction `f64::sqrt` compiles to, so every
  result is bit-for-bit the interpreter's, `-0.0` keeps its sign and a NaN is
  only a NaN (an unordered `ucomisd` is never a negative). A negative value is
  the interpreter's `E111`, whose sentence quotes the offending number: the
  runtime renders it with the writer `print` uses (the int writer for an int,
  the float formatter for a float), writes the sentence without the line
  clause a compiled call cannot carry, and exits 1, so a compiled program never
  goes on with a value the interpreter refuses to produce. A wrong arity or a
  non-number argument keeps the interpreter's `E110`.
  `tests/native/fixtures/sqrt.akk` covers both optimiser modes, and
  `a_negative_square_root_panics_like_the_interpreter` pins the `E111` line of
  an int, a float and a parameter.
- A tuple's shape is now per position. The analyzer's flat element shape (a
  chain of kinds) became a tree -- `Shape::Scalar`, `Seq` (a list or set),
  `Dict` and `Tuple(positions)`, plus `Unsettled` and `Mixed` -- so a tuple no
  longer has to be one kind all the way through. `t[0]` is the int and `t[1]`
  the string of `(1, "a")`, an `enumerate` pair is read at both positions even
  when the source mixes kinds, and `entries(d)`'s key and value are both typed.
  An indexing step resolves a constant position; any other index answers what
  all the positions agree on, and a mixed position is the usual run-time read.
  A store into a tuple updates the position it names, `reverse` reverses them
  with the elements, and a parameter or a returned tuple keeps them across a
  call (`Shape::refines` lets a later round tighten an unsettled shape). The
  runtime is untouched: every element word already carries its tag, so a
  positionally typed read is the same word read.
  `tests/native/fixtures/positional.akk` covers both optimiser modes.
- `enumerate(xs)` in compiled programs: one `(index, value)` tuple per element
  of a list, tuple or set, in order, through `rt_enumerate_seq`. The index is an
  int and the value keeps its own stored tag, so a heterogeneous collection
  enumerates too, and an empty list answers an empty list. A pair is an ordinary
  tuple, so it prints, compares and iterates, and (since the shape became
  positional) both halves of a pair are read by index. `enumerate` of a string
  counts grapheme clusters, which needs the syllable table, so it is refused
  with `E177`; a dict keeps the interpreter's `E110`.
  `tests/native/fixtures/enumerate.akk` covers both optimiser modes.
- `replace(text, from, to)` in compiled programs: the text with every
  non-overlapping occurrence of `from` replaced by `to`. The runtime counts the
  occurrences, allocates the exact result
  (`len(text) - count * len(from) + count * len(to)`) and copies the pieces
  around each replacement through `rt_replace_str`, which is the interpreter's
  `str::replace`. An empty `from` answers the text unchanged, the interpreter's
  own rule, so no Unicode table is needed; a Myanmar piece is replaced wherever
  its bytes appear. The interpreter's `E110` is kept for a wrong arity and for
  a non-text argument, and a value whose kind is only known at run time is
  refused with `E177`. `tests/native/fixtures/replace.akk` covers both
  optimiser modes.
- `starts_with(text, part)` and `ends_with(text, part)` in compiled programs:
  the part's bytes are compared with the front (or back) of the text through
  `rt_starts_str`/`rt_ends_str`, which is the interpreter's
  `str::starts_with`/`str::ends_with`. An empty part is `True`, a part longer
  than the text is `False`, and no Unicode table is needed -- a prefix of a text
  is still text, so a Myanmar prefix matches while a piece that cuts into the
  text does not. The interpreter's `E110` is kept for a wrong arity and a
  non-text argument, and a part whose kind is only known at run time is refused
  with `E177`. `tests/native/fixtures/starts_ends.akk` covers both optimiser
  modes.
- `join(xs, sep)` and `split(text, sep)` in compiled programs. `join` spells
  each element of a list, tuple or set the way `print` does -- by the element's
  stored tag, so a heterogeneous collection joins -- and concatenates the
  pieces with the separator into one fresh arena string, through `rt_join_seq`;
  an empty collection is the empty string. `split` is the interpreter's
  byte-wise `str::split`: it counts the non-overlapping occurrences, allocates
  the list at its exact size and copies one string per piece, through
  `rt_split_str`, so `"a,,b"` keeps its empty middle piece and a Myanmar
  separator matches wherever its bytes appear. A `join` of a collection of
  collections (which would need the interpreter's nested rendering) and a
  `split` on an empty separator (which means grapheme clusters) are refused
  with `E177`: a literal empty separator at build time, one that only turns out
  empty at run time with a documented `E177` exit. The interpreter's `E110`
  wording is kept for a wrong arity, a non-text separator, a first argument
  that is not a list/tuple/set, and a `split` of non-text.
  `tests/native/fixtures/join_split.akk` covers both optimiser modes.
- `contains(hay, needle)` in compiled programs: over a list, tuple or set it
  asks whether any element equals the needle, over a dict whether it is a key,
  and over text whether the needle's bytes are a substring. The collection scan
  runs through the runtime's own `rt_value_eq` (`rt_contains_seq`,
  `rt_contains_dict`), so a heterogeneous collection answers by each element's
  stored tag; text is a byte scan (`rt_contains_str`), which is what the
  interpreter's `str::contains` does, so a Myanmar substring is found wherever
  its UTF-8 bytes appear and an empty needle is found. `contains` of something
  that is neither text nor a collection, a text search for a non-text value and
  a wrong arity keep the interpreter's `E110`; a value whose kind is only known
  at run time is refused with `E177` by name.
  `tests/native/fixtures/contains.akk` covers both optimiser modes, and
  `contains_arity`/`contains_of_int`/`contains_text_needle` pin the `E110`
  refusals.

### Changed

- Version bumped to `3.1.3-alpha.3`.
- The `E177` summary now names `sqrt` beside `abs`/`min`/`max` and the
  rounding builtins.

### Fixed

- Element equality now compares both tags. `rt_value_eq` used to look at the
  left tag only, so `[0] == [False]` and `[0.0] == [0]` answered `True` where
  the interpreter answers `False`, and a set built at run time from `0` and
  `False` dropped one of them. The helper takes the right tag in `rsi` and all
  five call sites (collection equality, the run-time set member and the set
  write) pass it, so a value of another kind is never equal however its word
  looks.

## 3.1.2-alpha.3

### Added

- `slice(xs, start[, end])` in compiled programs, over a list, tuple or set.
  The runtime copies the selected elements' `{ tag, word }` pairs into a fresh
  list through `rt_slice_seq`, so a heterogeneous collection slices and the
  result keeps each element's own tag and nested shape; a tuple or a set slices
  into a list, which is what the interpreter answers for every source. The
  interpreter's bounds rules are kept: both ends clamp to the length, a start
  past the end is an empty result, and only a negative start or end fails --
  the interpreter's `E110`, written without the line clause a call cannot
  carry. A dict is not a sequence (its `E110`), text slices by grapheme cluster
  and needs the Unicode tables (refused with `E177`), and a wrong arity keeps
  the interpreter's `E110` wording. `tests/native/fixtures/slice.akk` covers
  both optimiser modes, `slice_arity`/`slice_of_dict` pin the `E110` refusals,
  `string_slice` pins the `E177`, and
  `a_slice_with_a_negative_bound_panics_like_the_interpreter` checks the
  panic's exact sentence.

### Changed

- Version bumped to `3.1.2-alpha.3`.
- The `E177` slice summary now names `slice` beside the other collection
  builtins.

## 3.1.1-alpha.3

### Added

- `values(d)` and `entries(d)` compile beside `keys(d)`: the runtime copies the
  dict's stored tags into a fresh list, so both views print, count, index,
  compare and iterate as ordinary lists, and an entry is a `(key, value)`
  tuple whose value keeps its own tag. `tests/native/fixtures/dict_views.akk`
  covers both optimiser modes, and `values_of_list`/`entries_arity` pin the
  interpreter's `E110`.
- Heterogeneous collections (`[1, "two", 3.5, True]`) compile: the descriptor
  already stores a tag beside every element word, so printing, `len`,
  equality, copying, an element store and `push`/`insert` all read each
  element's own tag. The analyzer's element shape gained `Dynamic` for exactly
  this case; an element *read* has no static kind and is still refused with
  `E177` by name. `tests/native/fixtures/dynamic_collections.akk` pins the
  supported forms and `dynamic_read`/`dynamic_iteration`/`dynamic_pop` the
  refusals.
- A set with a run-time member or a dict with a run-time key allocates its
  descriptor once and fills it in place through the new `rt_fill_set` and
  `rt_fill_dict`, keeping the interpreter's insertion-ordered deduplication
  and a key's first position / last value. A literal rebuilt in a loop no
  longer allocates (and copies) once per member, which is what could exhaust
  the fixed arena for a small literal;
  `a_runtime_member_literal_allocates_once_per_evaluation` pins it.

- An element read out of a heterogeneous collection compiles: the temporary
  keeps the address of the element's own `{ tag, word }` pair, so printing
  dispatches on the stored tag through `rt_print_dynamic`, and a literal, set
  member or dict value copies both words with `Gen::load_tagged`. The value
  keeps its kind through a binding, a function parameter and a new literal;
  `str`, arithmetic, comparison, a condition, iteration and
  `pop`/`remove`/`sort`/`sum` on it are refused by name with `E177`.
  `tests/native/fixtures/dynamic_reads.akk` covers both optimiser modes and
  `dynamic_str`/`dynamic_arithmetic` pin the refusals.

### Changed

- Version bumped to `3.1.1-alpha.3`.
- `docs/COLLECTIONS.md` now says mixed-kind element stores are supported (only
  an element read is refused), as Step 15 landed.

## 3.1.0-alpha.3

### Added (heterogeneous collections)

- A collection whose elements are not all one kind now compiles. The runtime
  descriptor already carried a tag beside every element word, so a literal
  such as `[1, "two", 3.5, True]` is one ordinary descriptor: printing, `len`,
  equality, copying and iteration-free use all read each element's own tag, and
  the output is the interpreter's, byte for byte. The analyzer's element shape
  gained a `Dynamic` marker for exactly this case.
- An element write and a `push`/`insert` of another kind widen the element
  shape to `Dynamic` instead of being refused, at any nesting depth: a list of
  int lists takes a list of strings (`grid[0] = ["nine"]`), and an inner list
  that started empty settles from the value written into it.
- A set with a run-time member (`{n, 2}`) and a dict with a run-time key are
  now built into one pre-sized descriptor: the code generator allocates room
  for every written member once, and `rt_fill_set`/`rt_fill_dict` fill it in
  place with the interpreter's insertion-ordered deduplication and a key's
  first position / last value. A literal rebuilt inside a loop therefore costs
  one allocation per evaluation instead of one per member (each of which also
  copied the whole collection so far), which is what used to exhaust the fixed
  arena for a small literal in a long loop.
  `a_runtime_member_literal_allocates_once_per_evaluation` in
  `tests/native/main.rs` loops over both literal forms enough that the old
  per-member allocation would have exhausted the 256 KiB arena.
- A read of a `Dynamic` element (indexing or iterating the collection), and a
  builtin that answers one element of it (`pop`, `remove`, `sort`, `sum`), are
  refused with `E177` by name: the value's kind is only known at run time, and
  compiled code must not guess it. `tests/native/fixtures/dynamic_collections.akk`
  pins the supported forms in both optimiser modes, and `dynamic_read`,
  `dynamic_iteration` and `dynamic_pop` pin the refusals.

### Added (dict views)

- `values(d)` and `entries(d)` now compile beside `keys(d)`. A dict descriptor
  already stores each value's own tag, so the runtime copies the pairs into an
  ordinary list: `values` copies every value word with its tag, and `entries`
  builds one `(key, value)` tuple per pair, in insertion order. The result
  types as a list, so it prints, counts, indexes, compares and iterates with
  the rest of the collection slice; a dict of strings keeps every entry
  element a string (so an entry position is readable), a dict of lists keeps
  its nested shape readable, and a tuple of mixed kinds settles as `Dynamic`
  exactly as a written tuple of the same kinds would. A non-dict argument and
  a wrong arity reproduce the interpreter's `E110`, without the line number
  compiled code cannot carry. `tests/native/fixtures/dict_views.akk` pins the
  supported forms in both optimiser modes, and `values_of_list` and
  `entries_arity` pin the `E110` refusals.

### Added

- `push`/`append` in compiled programs. The first argument has to be the
  variable that names a list, as in the interpreter (a value is a copy, so
  appending to a literal could not write anything back). The append itself is
  the collection runtime's indexed write with the index set to the descriptor's
  length word, so it grows the list by allocating a descriptor of one more
  element, copying the old elements and adding the value -- the same code an
  `xs[len(xs)] = v` store uses. The binding takes the fresh descriptor, which
  keeps the interpreter's value semantics (a second binding of the same list
  still prints its own contents), and the builtin answers the new length.
  `push` joins the list's element shape, so a value of another kind is refused
  with `E177` and the same wording an element write of it gets; a wrong arity,
  a first argument that is not a variable and a non-list value reproduce the
  interpreter's `E110`, minus the line number compiled code cannot carry.
  `tests/native/fixtures/list_builtins.akk` covers the returned length, a list
  growing inside a `while` loop, a mixed float list, a nested list, value
  semantics across a second binding, and both build modes.
- `reverse` in compiled programs. A list or tuple becomes a fresh descriptor
  with its elements in the opposite order, built by the new `rt_reverse_seq`
  (allocate, copy the header, place each element at `len - 1 - i`; the source
  is never written). A list held in a variable is reversed in place -- the
  binding takes the fresh descriptor -- while any other list or a tuple is
  reversed as a value and its binding stays as it was, exactly as the
  interpreter's `call_reverse` does. Reversing text is still refused with
  `E177` because it goes by grapheme cluster and needs the Unicode tables; a
  wrong arity or a value that is neither a list nor a tuple keeps the
  interpreter's `E110`. `list_builtins` also covers a tuple and a list literal,
  and `push_arity`/`reverse_arity` pin the two `E110` cases.

### Added (sum and keys)

- `sum(xs)` in compiled programs, over a list, tuple or set of numbers. The
  element shape the analyzer already tracks picks the result: an int list sums
  into a double and comes back through the interpreter's saturating `as i64`
  cast (`sum([9223372036854775807, 1])` is `9223372036854775807`), a float list
  answers the double. The walk is emitted in line: the loop adds each element
  word as a double, in descriptor order, which is the interpreter's `f64`
  accumulation. A collection of another element kind, or one whose element
  kind never settled, is refused with `E177` by name instead of compiled into
  an `E110` the interpreter would report at run time.
- `keys(d)` in compiled programs: a dict's keys, in insertion order, as a
  list. It is the same list a `for` loop over the dict already built
  (`rt_items_dict`), so the builtin is that call and needs no new runtime
  code; the analyzer types the result as a list of strings, which makes
  printing, indexing, `len` and iteration over it work. A non-dict argument is
  refused with `E177`.
- `tests/native/fixtures/reducers.akk` covers both in both optimiser modes: an
  int list, a float list, a tuple, a set, a literal and a one-off int list that
  saturates, `sum` through two parameters (one int call site, one float), and
  `keys()` printed, indexed with `len` and walked by `for`.

### Added (catchable arithmetic errors)

- A zero divisor for `/` or `%` now unwinds like a raise instead of ending the
  process, so the interpreter's `E036`/`E037` (division) and `E038`/`E039`
  (remainder) are catchable in a compiled program. Lowering emits a new
  `GuardZero` instruction before each of those operators, carrying the
  operator's line and the block the error travels to (the innermost `try`, or
  the function's escape), and codegen picks the code from the operand kinds the
  way the interpreter does: two ints give the int code, any float operand the
  float one, and a `NaN` divisor is not zero. The message is the interpreter's
  own, with its line number; the identity slot holds the code and no prefix is
  set, because a panic is not a named raise.
- The unwinding protocol now treats the identity slot as the pending flag, so a
  call that panics unwinds out of a callee exactly as a raise does:
  `check_error` tests the identity, `clear_error` empties it, and a clause whose
  error has no prefix binds the code (`E036`) rather than a message. `E178`
  takes a raise only -- a panic has no prefix -- while `E` takes both, matching
  the interpreter's `error_matches`. `rt_report_error` skips an empty slot, so
  an uncaught panic prints just its message and exits 1.
- A clause may name an interpreter code only when the program can produce it:
  `/` puts `E036` and `E037` in the set a clause may select, `%` puts `E038`
  and `E039`; a clause for any other code (`E104`, `E049`) is still refused
  with `E177`.
- `HirExpr::Binary` keeps the operator's line, which lowering had been dropping;
  that is what lets the native message match the interpreter's byte for byte.
  The unused `rt_panic_div_int`/`rt_panic_div_float` routines are gone, and a
  compound `x %= 0` still reports `E038` through `rt_panic_zero` (it has no
  guard and no line).
- Parity moved to **15 match / 41 refuse**: `div_zero` and `mod_zero` now build
  and match the interpreter's stdout, stderr (line numbers included) and exit
  status. `tests/native/fixtures/arith_catch.akk` covers `E` binding `E036`, a
  named `E037`/`E038`/`E039` clause, a `finally` on a panic, `E178` letting a
  panic through, and an uncaught panic's exact line, in both optimiser modes.

### Added (str conversion and interpolation)

- `str(x)` -- and its Myanmar spellings `စာ`/`စာသား` -- in compiled programs.
  An int becomes its decimal text (`rt_str_int`), a bool one of the interned
  `True`/`False` descriptors the printer already uses (`rt_str_bool`), a
  string is itself, and a float reuses the exact print formatter: the
  formatter's final step now copies its rendered bytes into a fresh arena
  string when a `.data` flag says `rt_str_float` is running, instead of
  writing them to stdout. So `str(2.5)` is `2.5`, `str(3.0)` is `3.0` and a
  whole, zero, negative or very large value keeps the interpreter's spelling.
- `{name}` interpolation inside a string literal, in both spellings. Lowering
  reads the literal exactly as `interpolate_text` does (`\{`/`\}` are literal
  braces, a trimmed name between braces, placeholders in source order) and
  builds `str(name)` pieces joined with `+`, so interpolation and the explicit
  conversion share one implementation and one display rule. `MIR` gained a
  `ToStr` instruction (rather than a `Call`, so a user function named `str`
  cannot shadow it), the analyzer types it as a string and refuses a
  collection argument, and the optimizer passes carry it.
- `tests/native/fixtures/stringify.akk` covers `str()` of positive, negative
  and zero ints, a fractional, whole and `-0.0` float, both bools and a
  Myanmar string; interpolation with the same values, an escaped brace pair, a
  concatenated conversion, and a parameter filled by a string and by an int
  call site, in both optimiser modes. The parity corpus moved to **13 match /
  43 refuse**: `native_interpolation`, `eng_print` and `switch_forms` now
  compile and match the interpreter byte for byte.

### Added (switch)

- `switch` in compiled programs, in both the eng and the Myanmar spelling.
  Lowering turns it into a `Move` of the subject, one `Compare`/`Branch` per
  `case` in written order, and a jump per arm body, so no new MIR instruction,
  backend rule or runtime helper was needed. The interpreter's evaluation
  order is kept exactly: the subject once, the case expressions in written
  order until one is equal to it (a later case is never evaluated, so its
  side effects never happen), the first matching arm only, and the `default`
  arm only when no case matched -- wherever that arm is written. A float
  subject never equals an `int` case, because the comparison is the
  language's own `==`. A `break` inside an arm leaves the switch and not the
  loop around it: lowering keeps a break-target stack separate from the
  continue targets, so a `continue` in an arm still reaches the enclosing
  loop. `tests/native/fixtures/switch.akk` covers default-first, only the
  first match, no match without a default, a default-only switch, the
  evaluation order of the case expressions, a float-versus-int case, a nested
  switch, bool and string subjects, a `break` and a `continue` inside a loop,
  and the Myanmar form, in both optimiser modes.

### Added (list editing)

- `pop`, `insert`, `remove` and `sort` in compiled programs. Each works on a
  fresh descriptor the collection runtime builds and rebinds the list variable
  to, so a second binding keeps its own contents (value semantics).
  `rt_pop_seq` copies the list without its last element and answers that
  element, `rt_insert_seq` copies it with everything from the position shifted
  one place and answers the new length (the position may equal the length,
  which appends), `rt_remove_seq` copies it without the element at the
  position and answers that element, and `rt_sort_seq` is a stable insertion
  sort whose comparison -- int, float or `rt_cmp_str` for strings -- is picked
  from the element kind the analyzer settled. A list of any other element kind
  is refused with `E177`, because the interpreter orders those through its
  collection rules (`E042`).
- The two run-time failures these builtins can hit carry no line clause,
  because a compiled call carries no span (the rule the arithmetic panics
  already follow): adding `E110` for popping an empty list and `E049` for a
  position outside it. Compile-time misuse keeps the interpreter's `E110`
  wording, and `tests/native/fixtures/list_edits.akk` covers the returned
  values, all three insert positions, a discarded result, value semantics and
  int/float/string sorts in both build modes; `pop_empty` pins the panic and
  `insert_arity`/`sort_extra` pin `E110`.

### Changed

- The `E177` slice summary in a refusal now names `push`/`append`, `reverse`
  and `pop`/`insert`/`remove`/`sort`. Parity is **10 match / 46 refuse**: the
  corpus fixture `mutating` now compiles and matches the interpreter byte for
  byte, and `remaining_forms` still refuses at `reverse("abc")`.
- Version bumped to `3.1.0-alpha.3`.

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
