# ADR 0001: Native runtime value representation

- **Status:** Accepted (Step 2 landed; Step 3 collection slice in progress)
- **Date:** 2026-11-22
- **Scope:** the value word the native runtime and codegen will exchange

> The plan's Step 2 names "2.1 ADR / spike" but does not give the ADR a path.
> This file is that ADR: `docs/adr/0001-runtime-value-representation.md`.
> The tested runtime layer is `src/runtime/alloc.rs` (bump allocator and GC
> headers) and `src/runtime/value.rs` (the word, heap payloads, constructors,
> accessors, and display/equality/condition dispatch). Step 3's first slice now
> makes native codegen use the tag vocabulary and collection display rules from
> that layer; the interpreter and VM are untouched, and byte-identical output
> is still the parity gate.

## Context

The current native backend has a static four-kind model (`int`, `float`,
`bool`, `string`) and refuses everything else with `E177`. Step 2 replaces
that with one runtime value representation that can grow to every Akkhara
value kind without changing the instruction shape at every later step:

- `null` / `ဗလာ`
- `bool`
- `int` (a full signed 64-bit integer)
- `float` (IEEE-754 binary64, exact spelling preserved)
- `string`
- `list`, `tuple`, `set`, `dict`
- class objects
- function values and closures
- pending errors / raised values (the runtime error ABI)

A value word is copied constantly: into temporaries, frame slots, argument
registers and collection storage. The representation therefore has to be
cheap to move and unambiguous at run time, because the native backend has no
type tags in its machine code after this step.

## Decision

Use a **64-bit NaN-boxed value word**. In the spike `Value` wraps one `u64`,
so a value is one register and one 8-byte stack slot.

### Bit layout

```text
 63  62 .. 52  51  50 .. 47  46 .. 0
  1  1 1 1 ... 1  1    tag      47-bit payload
```

- Bits 63..51: sign = 1, exponent = all ones, quiet bit = 1. This is the
  negative quiet-NaN space.
- Bits 50..47: a 4-bit tag (16 tags, 11 used).
- Bits 46..0: a 47-bit payload. For heap kinds it is an address; for
  tag-only kinds it is zero.

Detection is one mask:

```text
TAGGED   = (word & 0xFFF8_0000_0000_0000) == 0xFFF8_0000_0000_0000
TAG      = (word >> 47) & 0xF
PAYLOAD  = word & 0x0000_7FFF_FFFF_FFFF
```

Everything else is an ordinary binary64 float.

### Float canonicalisation

Any NaN produced by arithmetic or parsed from source is stored as the single
canonical quiet NaN `0x7FF8_0000_0000_0000` (positive quiet NaN). This is the
only NaN pattern a float value may hold. Payload bits of NaNs are never
observable in Akkhara today (printing says `NaN`), so folding them loses no
behavior. `-0.0`, infinities and every finite double keep their exact bits.
This is what makes the negative-NaN tag space exclusive: no float can set the
tag mask, including a negative NaN from hardware.

### Encoding of every kind

| Kind | Tag | Payload | Notes |
| --- | --- | --- | --- |
| `null` | 0 | 0 | tag only |
| `false` / `true` | 1 / 2 | 0 | two tags, no payload |
| `int` | 3 | pointer to `i64` | full i64 range; boxed (see below) |
| `string` | 4 | pointer to `String` | UTF-8, length inside the allocation |
| `list` | 5 | pointer to `Vec<Value>` | ordered, may hold every kind |
| `tuple` | 6 | pointer to `Vec<Value>` | fixed length by construction |
| `set` | 7 | pointer to `Vec<Value>` | spike storage; the production set adds a hash index |
| `dict` | 8 | pointer to `Vec<(Value, Value)>` | spike storage; production adds a hash index |
| `object` | 9 | pointer to `Object` | class name + ordered `(field, Value)` list |
| `function` | 10 | pointer to `Function` | name + arity; closures extend it |
| reserved | 11..15 | — | error/pending value and iterator slot planned |

Every tag is distinct; `src/runtime/value.rs` asserts that in a test.

### Alignment and pointer-boxing constraints

- Heap payloads are **8-byte aligned**. The low 3 payload bits are therefore
  always zero and are available for a future pointer-tag optimisation.
- Pointers must fit in **47 bits**. This is true of Windows x86-64 user space
  (47-bit) and of Linux x86-64 with 4-level paging (47-bit canonical user
  addresses). Linux may be built with 5-level paging (57-bit virtual
  addresses); in that configuration the allocator must serve runtime objects
  from an arena below `2^47` (for example `MAP_32BIT`/a reserved heap region),
  or this representation must not be used. 32-bit targets are outside this
  design: values are 64-bit and pointers would still fit, but the native
  backend only targets x86-64 today.
- Allocation must not hand out addresses with the tag mask set. A 47-bit
  user-space arena guarantees that.
- A non-null heap pointer is never zero, so a zero payload is always a
  tag-only encoding; the spike asserts this for every heap kind.

### Ownership, GC and runtime ABI

- `Value` is `Copy`: copying a value copies its bits and does not touch the
  heap. The interpreter's reference-counted values do **not** define the
  native ABI.
- `src/runtime/alloc.rs` implements the bump allocator: payloads are carved
  from 64 KiB chunks, 8-byte aligned, and every allocation is registered. A
  payload starts with `Header { kind: HeapKind, marked: Cell<bool> }`, so the
  collector can recover its type from the pointer; `Heap` exposes the object
  registry, mark/clear operations and chunk statistics.
- The heap payloads are real, typed structs (`HeapInt`, `HeapString`,
  `HeapList`, `HeapTuple`, `HeapSet`, `HeapDict`, `HeapObject`,
  `HeapFunction`) in `src/runtime/value.rs`, each with the header in its first
  `#[repr(C)]` field. Strings store UTF-8 bytes with the vector's length and
  capacity; list/tuple/set store `Vec<Value>`; dict stores ordered
  `(Value, Value)` pairs; objects store class + fields; functions store name +
  arity.
- Allocation never frees individual objects in this step, as the plan allows
  ("memory growth permitted if GC is not included yet"). Dropping the `Heap`
  drops every payload and its owned buffers, then the chunks. The mark bit and
  registry are in place for mark-sweep, but the traversal/collection cycle is
  deferred to Step 3, where compiled programs first allocate in loops.
- Rooting at a GC safepoint will use a shadow stack / root table the codegen
  maintains for temporaries and frame slots; a value in a machine register is
  not a root by itself. That belongs to the codegen migration, deferred to
  Step 3.
- `int` is boxed in this design. That costs one allocation per non-small
  integer and one indirection per arithmetic operation until a fast path is
  added. The ADR accepts this for the first dynamic-value step because a
  full `i64` cannot fit in 47 payload bits. A future revision may add a small
  inline-int tag (for example a 47-bit signed immediate) while keeping the
  boxed tag for the full range; that is an optimization, not an ABI break,
  because both decode to the same `i64`.
- Strings and collections are heap values; display/repr, equality and
  condition dispatch read the tag, not a static kind. `Value::display`,
  `Value::repr`, `value_eq` and `Value::condition` in `src/runtime/value.rs`
  mirror the interpreter's rules exactly (integral floats print `1.0`, strings
  print raw but quote inside collections, `1` is not equal to `1.0`, `NaN`
  equals `NaN` through the representation rule, and only a real bool is a
  condition). String concat, byte-range slice and lexicographic compare are
  implemented on `Heap`/`Value` with tests. The existing `E037`/`E038` panic
  behavior and `E178` raise prefix stay byte-identical because their text
  already comes from `src/shared/errors.rs`.

## Rejected alternatives

### 16-byte `{ tag: u64, payload: u64 }`

Rejected for the first dynamic step even though it is simpler:

- It doubles every temporary and frame slot (8 → 16 bytes) and needs register
  pairs for movement, which would rewrite the current one-register slot ABI
  in the same step that introduces dynamic values.
- It cannot use the negative-NaN trick, so every float copy carries a separate
  tag word; the common numeric paths become two-word moves.
- The simplicity advantage (no int boxing, no NaN canonicalisation) is real,
  but it is bought with memory traffic on every value, including the numeric
  programs that are the backend's current strength.

It remains the fallback if 47-bit pointer constraints turn out to be
unworkable on a supported target.

### Inline 47-bit integers in the NaN tag space

Rejected: it would silently truncate `i64` values and change overflow and
printing behavior for large integers. The current interpreter uses the full
`i64` range, and parity requires the same answers.

### Pointer tagging with low bits (no NaN space)

Rejected: arbitrary doubles are valid values and occupy every bit pattern, so
a low-bit tag would make some floats indistinguishable from pointers. It also
requires every numeric operation to mask and test.

### Tagged `enum Value` in Rust

Rejected as the runtime representation: a Rust enum is an implementation
detail, not a stable machine ABI, and its layout (niche optimization included)
is not guaranteed across compiler versions. It is excellent for the
interpreter, which is why the interpreter keeps its own `Value` enum; the
native runtime needs an explicit word layout.

### Boxed-everything (every value is a heap pointer)

Rejected: every float and bool would allocate and every arithmetic operation
would chase a pointer. This is the slowest option and loses the ability to
keep numbers in registers.

### 128-bit NaN-boxing

Rejected: no target ABI or register file benefits from 128-bit values here,
and it doubles slot/frame sizes for no additional kind capacity.

## Consequences

- The native backend can migrate its static `Kind` model to one word without
  changing the runtime call ABI again for later kinds.
- Integer-heavy code pays an allocation/indirection cost until the small-int
  fast path; this is explicitly deferred.
- The GC must know which words in frames and collections are heap pointers.
  The tag makes that possible without a side table.
- 5-level-paging Linux and any target with pointers above `2^47` require an
  arena constraint, documented above.
- NaN payload preservation is intentionally not part of the language
  semantics; only `NaN` identity and printing matter, and both are preserved.

## Implementation status

Landed and proven by tests:

- `src/runtime/alloc.rs`: the bump allocator (64 KiB chunks, 8-byte alignment,
  chunk growth, stable payload addresses), `Header { kind, marked }`, the
  object registry, mark/clear operations and chunk statistics.
- `src/runtime/value.rs`: the word layout, canonical NaN handling, tag
  detection, pointer alignment checks, all typed payloads and their
  headers, constructors/accessors for every kind, display/repr/equality/
  condition dispatch, and string concat/slice/compare helpers.
- Ten unit tests: every-kind round trip, 8-byte size/alignment, distinct tags,
  header kinds for every heap value, display/repr vs the interpreter, equality
  vs the interpreter's representation rule, bool-only conditions, string
  helpers, bump-allocation stability/chunking, and header registration/marks.
- The plan's `panic.rs` item needs no extension: the native ABI already
  carries the raise prefix, identity and payload in the three slots the
  interpreter's `E178` line needs (see `src/runtime/raise.rs`).
- Organization note: the plan names `string.rs`/`list.rs`/`dict.rs` helpers;
  the typed Rust payloads live together in `value.rs` because they share the
  tag/header machinery. The existing `string.rs` remains the machine-code
  `rt_cmp_str` emitter and is untouched. This is a file-layout deviation, not
  a representation change.

### Step 3, first collection slice (landed)

The native backend now emits collections, so this slice is the codegen bridge
the ADR anticipated. It deliberately does **not** yet make every slot a
NaN-boxed word; it uses the same tag vocabulary and display rules, with a
concrete descriptor layout the rest of the migration can replace:

- `Kind` grew `List`, `Tuple`, `Set` and `Dict`, and the analyzer learns the
element kind of a collection (and carries it through `StoreVar`) so `xs[0]`
reaches the same `Kind` the interpreter's value would.
- A collection value is one pointer to a writable descriptor:
  `{ tag, length, [tag, word] * length }` (`[key_tag, key, val_tag, val]` for a
  dict). The tags are the `TAG_*` constants in `src/runtime/value.rs`, plus two
  descriptor-only tags (`TAG_FLOAT`, `TAG_BOOL`) where the word decides how to
  print. `Kind::tag` in `src/backend/x86_64/mod.rs` is the single mapping.
- `src/runtime/collections.rs` emits the runtime side in machine code:
  `rt_alloc` (bump allocation over a 256 KiB zeroed arena in the image),
  `rt_index_seq`, `rt_index_dict`, `rt_print_value`/`rt_print_collection`, and
  the `E049`/`E053` diagnostics. `MirInst::Index` carries its lowered source
  line, so those two diagnostics are byte-identical to the interpreter's
  (the older E036/E037/E038 panics still omit a line). The printers reuse the
  existing int/float/bool/string writers through new no-newline entry points.
  Exhausting the arena takes an honest error path: `rt_alloc` writes
  `E177 native runtime arena ကုန်ဆုံးသွားပါသည်။` to stderr and exits 1; a
  native test builds a literal past 256 KiB and observes exactly that.
- Constructors write the descriptor the analyzer typed; list/tuple/set/dict
  printing matches the interpreter, including nested collections, quoted
  strings inside collections, and `1.0` float spelling. Set members and dict
  keys are deduplicated at compile time: a run-time set member or dict key is
  refused rather than silently keeping a duplicate.
- `len(collection)` and `MirInst::Len` read the descriptor's length.

### Step 3, iteration slice (landed)

`MirInst::Items` is now emitted for the collections the slice carries, so a
`for` loop over one compiles:

- A list, tuple or set loop walks the collection itself: `Items` copies the
descriptor and the existing `Len`/`Index` shape reads it. The analyzer carries
the element kind through `Items` into the loop variable, so the body is typed
exactly like an `xs[i]` read.
- A dict loop yields its keys in insertion order, which is what the
  interpreter's `Value::Dict` arm does. The runtime helper `rt_items_dict`
  copies the key words into a fresh list descriptor (every key in the slice is
  a string literal), and the same `Len`/`Index` shape then walks that list.
- Iterating a string (grapheme clusters) and a number (interpreter auto-range)
  are still refused by name, as is a loop whose element kind does not settle
  (an empty literal, or a collection that reached the loop through a
  parameter, whose element kind does not cross calls yet).
- Parity moved to 7 match / 47 refuse because `control_flow` -- a range loop,
a collection loop and a `while` -- now compiles and matches the interpreter
byte for byte; that is the measured effect of the slice, not a fixture being
relaxed. `collections` still refuses on the `စာ` conversion and `shorthand` on
the `map` callback, with their refusal keywords naming the remaining
constructs.

### Step 3, index-mutation slice (landed)

`MirInst::StoreIndex` (`xs[i] = v`) is now emitted for every collection kind
the interpreter allows:

- The analyzer learns the index kind (`Int` for a list/tuple/set position,
`Str` for a dict key) and the value kind from the base collection's
  element/value descriptor tag, writes it back when the collection was empty,
  and refuses a string base, a mismatched value kind and a base that is not a
  variable with `E177` naming the case.
- **Value semantics are preserved by copying.** The interpreter's `List` and
  `Tuple` hold their elements inline and `assign_index` writes a fresh value
  back into the name, so `ys = xs; ys[0] = 9` leaves `xs` unchanged. A native
descriptor is a pointer both bindings would share, so the runtime helpers
  (`rt_store_seq`, `rt_store_dict`, `rt_store_set`, with `rt_value_eq`) build
  a **new** descriptor and codegen rebinds the variable's slot to it. The old
  descriptor stays in the arena (no collector yet) and the interpreter's
  observable behaviour -- including dict/float/string set dedup rules -- is
  reproduced.
- Sequence writes take `0..=len`: a position inside the collection is
  replaced, the position just past the end appends. `rt_store_seq` reports the
  interpreter's `E049` (line included) before 0 or past the end. A dict write
  updates the key where it sits or appends a new key last; a set write follows
  the interpreter exactly: the member already at the slot changes nothing, a
  member that lives elsewhere drops the written position, the position just
  past the end appends, anything else is replaced.
- The write's source line travels in `r8` (new `StoreIndex::line`), so the
  `E049` text is byte-identical to the interpreter's.
- Parity stays 7 match / 47 refuse: no fixture in `tests/fixtures/` writes
  through an index. The native-only fixture `mutation.akk` covers the landed
  forms and is compared with the interpreter under both optimiser modes; the
  `collection_diagnostics_match_the_interpreter` test adds the write
  out-of-range cases, including the negative index.

Still deferred for this slice: heterogeneous collections (a write must match
  the element kind, as a mixed literal already does), writing a character of a
  string, and a nested write such as `grid[0][1] = v` (the parser rejects it).

### Step 3, collection-equality slice (landed)

`==` and `!=` between two collections now compile:

- The analyzer types the result as `Bool` and leaves the operand kinds alone.
  Equality is the interpreter's `repr(a) == repr(b)` rule, so two collections
  of different kinds (a list and a tuple, or a list and a dict) are unequal
  rather than a type error. Ordering a collection is the interpreter's `E042`:
  `Compare` now carries its source line, codegen evaluates both operands (their
  calls may print) and then calls `rt_panic_message` with the fully built
  `E042 လိုင်း <n> တွင် <kind> နှင့် <kind> ကို "<op>" ဖြင့် နှိုင်းယှဉ်၍မရပါ။`
  text, so stderr and the exit status are byte-identical. The same slice fixed
  `Kind::mm_name` for tuple (`အစု`) and set (`အုပ်စု`) to match the
  interpreter's `type_name_mm`.
- `rt_collection_eq` compares kind, length and then every element in stored
  order -- for a dict, each key and its value -- through `rt_value_eq`, which
  now recurses into the nested collection tags. Order sensitivity is exactly
  what the printed form has, so two dicts with the same pairs inserted in a
  different order are not equal, as in the interpreter.
- A collection against a value of another kind is unequal without a runtime
  call (`!=` is its negation), the printed-form rule again.
- `tests/native/fixtures/equality.akk` covers lists, tuples, sets, dicts,
  nested collections, strings, floats, empty collections and the mixed-type
  cases, and is compared with the interpreter under both optimiser modes. Two
  unit tests type equality and ordering as `Bool`, and
  `collection_diagnostics_match_the_interpreter` adds the ordering cases
  (list/list, list/int, tuple/set) and compares stdout, stderr and exit code.
  Parity stays 7 match / 47 refuse because no fixture compares collections.
- New native fixture `tests/native/fixtures/collections.akk` is compared with
  the interpreter through `akk build` in both optimiser modes. Two native
  tests cover the rest: `collection_diagnostics_match_the_interpreter` asserts
  byte-identical `E049`/`E053` stderr for range, negative and missing-key
  cases, and `the_collection_arena_reports_exhaustion_honestly` observes the
  documented exhaustion path. Three unit tests cover index typing and the
  heterogeneous-list / run-time-set refusals.

### Step 3, nested-collection slice (landed)

A collection whose elements are themselves collections now compiles at any
depth:

- The analyzer's one-level element kind became an element **shape**:
  `[Int]` for a list of ints, `[List, Int]` for a list of int lists,
  `[Dict, Str]` for a dict of strings. `shape_of` prefixes an operand's kind,
  `items_shape` merges the element shapes of a literal, and `merge_shapes`
  allows a prefix (an empty literal's contents are unknown) while refusing a
  genuine disagreement such as `[[1], ["a"]]`.
- One indexing step (`learn_indexed`) gives the destination the first kind of
  the base's shape and carries the rest of the shape with the result, so
  `grid[0][1]` is typed as an int and `deep[0][1][0]` as an int through three
  levels. The same shape travels through `StoreVar` (`ys = xs`), `Items`
  (`for row in grid`) and dict values.
- A store compares the written value's shape with the target's
  (`learn_stored`), so a list of int lists refuses a list of strings instead
  of miscompiling the inner tag, and an empty collection adopts the shape of
  the value written into it. The runtime needed no new helpers: a nested
  element is already a descriptor pointer, so the existing index, print,
  equality and copy-on-write store paths read it unchanged.
- Refused (documented, never silent): an inner collection whose element kind
  is still unknown because it started empty (`[[]]` then `xs[0][0]`); a nested
  write, which the language's own parser rejects (`E001`); and a mixed nested
  literal (`[[1], ["a"]]`), which already fails the one-element-kind rule.
- `nested_collections_match_the_interpreter` writes four programs -- nested
  reads/prints/equality/stores over lists, three levels, a tuple of tuples and
  a dict of lists with iteration, plus a second-level `E049`, nested `E042`
  ordering and a failed nested store -- and compares stdout, stderr and exit
  code with the interpreter in both optimiser modes. A unit test
  (`a_nested_index_is_typed_from_the_inner_elements`) pins the shape typing.
  Parity stays 7 match / 47 refuse because no corpus fixture is nested.

### Step 4, compile-time string slice (landed)

`len(s)`, `for ch in s` and `s[i]` now compile when the text is known before
the program runs:

- The native runtime carries no Unicode tables -- `crate::text` lives in the
  compiler -- so the new `src/opt/strings.rs` pass asks it at compile time.
  For a literal, or a binding assigned one exactly once, `len(s)` becomes an
  int constant, `for ch in s` becomes a `List` literal of the string's
  grapheme clusters, and `s[i]` becomes an index into such a list. The pass
  runs before the analyzer and in both optimiser modes: it is lowering, not
  an optimisation.
- The rewritten forms reuse the collection slice unchanged, which is what
  keeps parity: iteration walks `[Str]`, and an out-of-range `s[i]` reaches
  the same `rt_index_seq` panic and the same `E049` text and line.
- A string that only exists at run time -- a parameter, a call result -- has
  no compile-time clusters. `len`, indexing and iteration over one stay
  refused with the `E177` messages they already had, covered by the
  `string_len`, `string_index` and `string_iteration` cases in
  `tests/native/main.rs`.
- `tests/native/fixtures/strings_ops.akk` covers ASCII and Myanmar `len`,
  empty text, a loop over a literal and over a binding, a literal, computed
  and empty-string index/loop, and `len` in arithmetic, under both optimiser
  modes. `collection_diagnostics_match_the_interpreter` adds `"မြန်မာ"[4]`,
  whose `E049` is byte-identical. The corpus fixture `native_strings` is the
  first string fixture to match, so parity moves to 8 match / 47 refuse;
  `eng_builtin` still refuses, now at `sqrt` because its `len("abc")`
  compiles.

### Step 5, numeric iteration slice (landed)

`for i in n` with a number now compiles, as the interpreter's auto-range:

- The analyzer types the loop source as a fresh `[Int]` list whatever the
  number's kind is, because the interpreter's counter is always an `int`
  (`0, 1, ...` while it is below the bound).
- `gen` calls `rt_items_range_int` with the bound. Its int arm passes the
  value straight through; its float arm runs the same saturating `ceil`
  conversion the `ceil` builtin uses, so `for i in 2.5` runs for 0, 1 and 2
  and a NaN bound runs no iterations. The runtime helper allocates a list
  descriptor of ints `0..n-1` (a bound at or below zero is an empty list) and
  the existing `Len`/`Index` loop walks it.
- An oversized bound (16384 elements already exceed the 256 KiB arena) is
  capped so `rt_alloc` takes its documented `E177` exhaustion path instead of
  the size arithmetic wrapping. The interpreter has no arena and would keep
  looping; the honest-exit rule is the same one the existing list-literal
  arena test documents.
- `tests/native/fixtures/number_for.akk` covers an int binding, a float
  bound, zero, a negative bound and a float reached through a parameter, in
  both optimiser modes. Parity stays 8 match / 47 refuse: no corpus fixture
  loops over a bare number.

### Step 6, string concatenation slice (landed)

`+` of two strings now compiles, the interpreter's one string arithmetic:

- The analyzer types the result as `str` and learns both operands as strings;
  a string beside a number stays refused (the interpreter reports `E040`),
  and a compound assignment on a string was already refused.
- `gen` pushes the left string, loads the right and calls `rt_concat_str`,
  which allocates `{ length, bytes }` for both ranges in the collection arena
  and copies them with `rep movsb`. Constant pairs are folded by
  `const_fold` when the optimiser runs; `--no-opt` reaches the runtime call.
- The new string lives in the arena like a collection, so a long chain of
  joins reaches the documented `E177` exhaustion instead of growing without
  bound; the interpreter has a heap and would keep going.
- One analyzer fix came with it: `default_numbers` no longer labels a `Binary`
  destination as `int` when an operand is already a non-number. Before, a `+`
  whose parameters were still unsettled was defaulted to int, and the call
  sites' `str` then contradicted it ("used as a int and as a string").
- `tests/native/fixtures/concat.akk` covers constant pairs (ASCII, Myanmar,
  empty), a left-associative chain, bindings and a binding growing through
  `+`, equality of a joined value, and a join through parameters, in both
  optimiser modes. The corpus fixture `native_concat` matches, so parity
  moves to 9 match / 47 refuse, and `defaults` now refuses later -- at the
  dynamic-scope name `seen` -- because its `name + punct` compiles.

### Step 7, element shapes across calls (landed)

A collection parameter and a returned collection now carry their element
shape across the call:

- A call site that passes a collection whose element shape is known records
  that shape on the callee's parameter, and a callee whose parameter shape is
  already known types the argument it receives. A read through the parameter
  (`xs[0]`, `grid[row][col]`, a store or a `for`) is then typed inside the
  callee, and its result kind is known at the call site.
- The result travels the same way in the other direction: a returned
  collection records its shape on the function, and a caller indexing the
  call's result learns its element kind -- including through a function that
  only returns its parameter, and through two calls in a row.
- The shape has to settle to one value, the way a kind does. A body that
  reads or writes one element kind while its call sites pass another is
  refused by name, and so are two call sites with different element shapes.
  That is the static slice's price; the interpreter answers from the value at
  run time.
- `tests/native/fixtures/collection_params.akk` covers a list, tuple, nested
  list and dict passed as parameters, a write and an iteration through a
  parameter, a returned literal indexed by the caller, and a shape crossing
  two calls (`identity`). `collection_param_mismatch` pins the two-call-site
  refusal, and the analyzer unit test
  `a_collection_shape_crosses_a_call_in_both_directions` pins the settled
  parameter and result shapes. Parity stays 9 match / 47 refuse: no corpus
  fixture passes a collection to a function before refusing for another
  construct.

### Step 8, float remainder slice (landed)

`%` with a float on either side now compiles:

- The interpreter promotes an int operand and computes `f64::rem_euclid`: the
truncated remainder (`fmod`) made non-negative. The analyzer types the result
from the promoted kind -- two ints keep the int remainder, anything with a
float in it is a float -- so a mixed pair is the float operation the
interpreter performs.
- SSE has no remainder instruction, so `src/runtime/rem.rs` emits
`rt_rem_float`: it checks the divisor for zero (a zero divisor is the
interpreter's `E039`; a NaN divisor merely returns NaN), pushes both doubles on
the x87 stack and runs `fprem` until C2 says the reduction is complete. The
true remainder is exactly representable as a double, so the store back to the
frame loses nothing. The result is then made non-negative by adding
`|divisor|` when it is below zero, which is `rem_euclid`'s rule and leaves
`-0.0`, NaN and the positive case alone.
- `%=` uses the same routine for a float binding and the existing int
`idiv`-based remainder for an int one; the encoder grew the six x87 opcodes
this needs (`fld`, `fstp`, `fstp st(0)`, `fprem`, `fnstsw ax`, `test ah, imm8`)
with a unit test pinning their bytes.
- `tests/native/fixtures/remainder.akk` covers int, float and mixed remainders,
the sign rules, a zero result, a call through a function and both `%=`
forms; `float_rem_zero` pins the `E039` panic (its message, minus the line
number compiled code has no span for, and exit 1). Parity stays 9 match / 47
refuse: the corpus fixture `mod_zero` now refuses later -- at its `E038` catch
clause, because compiled code can only catch a raise it makes itself -- so its
expectation keyword moved rather than the count.

Still deferred (and to which step):

- **Step 3:** `StoreField`, a string that is only
  known at run time (`len`/index/iteration over one),
  heterogeneous collections (a mixed literal or a write of another element
  kind), a collection whose parameter is given two different element shapes
  at different call sites, run-time set members and dict keys,
  mark-sweep traversal/collection, the shadow stack/root table, hash indexes
  for dict/set, and the full move from static `Kind` slots to one NaN-boxed
  `Value` per slot. The current bridge stores `int`/`float`/`bool` inline in
  slots and writes explicit element tags, so it is a layout step, not the
  final ABI.
- **Step 3 acceptance:** list/dict/object print fixtures (the interpreter's
  own `collections.akk`) and the million-iteration loop need the remaining
  constructs above; the new native fixture proves the subset that landed.
- **Step 4:** closure capture cells in `HeapFunction`, interpolation as a
  runtime string helper, and function values.

### Step 9, list builtins slice (landed)

`push`/`append` and `reverse` now compile:

- `push(xs, v)` requires its first argument to be the variable that names a
  list, as the interpreter does (a value is a copy, so `push([1, 2], 3)` could
  not write anything back). It appends by calling the runtime's indexed write
  with the index set to the descriptor's length word, so it reuses the same
  allocation, copy and `E049` code an element store does; the index is never
  out of range. The fresh descriptor the write returns replaces the binding --
  the interpreter's value semantics, so a second binding keeps its own
  contents -- and the builtin answers the new length. `append` is the same
  builtin, as in the interpreter.
- `reverse(x)` answers a fresh list/tuple descriptor with the elements in the
  opposite order, through the new `rt_reverse_seq`: it allocates the same size,
  copies the header and places each element at `len - 1 - i`. The source is
  never written. A list held in a variable is reversed in place -- the binding
  takes the fresh descriptor -- while any other list or a tuple is reversed as
  a value and its binding stays as it was, which is exactly the interpreter's
  `call_reverse` (only a `Value::List` in a variable is reversed in place).
  Text reverse is still refused: reversing a string goes by grapheme cluster
  and needs the Unicode tables.
- A `push` whose value does not fit the list's element shape is refused with
  the wording an element write of that value gets (`E177`), and the
  interpreter's `E110` is kept for a wrong arity, a first argument that is not
  a variable and a `reverse` over something that is not a list or tuple.
- `tests/native/fixtures/list_builtins.akk` covers the returned length, a list
  growing inside a `while` loop, value semantics across a second binding, a
  list variable reversed in place and returned, a tuple reversed as a value and
  a literal reversed into a new binding, with and without `--no-opt`.
  `push_mixed` pins the element-shape refusal, and `push_arity` and
  `reverse_arity` pin `E110`. Parity stays 9 match / 47 refuse: the corpus
  fixture `mutating` now refuses at its later `pop` -- so its expectation
  keyword moved rather than the count -- and `remaining_forms` still refuses at
  `reverse("abc")`.

### Step 10, list editing builtins slice (landed)

`pop`, `insert`, `remove` and `sort` now compile:

- All four follow the mutating-builtin rule the interpreter uses: the first
  argument must be the variable that names the list, so the runtime's fresh
  descriptor can replace it, and a second binding keeps its own contents
  (value semantics, exactly as `push` and the element write already did).
- `pop(xs)` copies the list without its last element (`rt_pop_seq`) and answers
  the dropped word; `insert(xs, at, v)` copies it with everything from `at`
  shifted one place and the value written into the gap (`rt_insert_seq`),
  answering the new length; `remove(xs, at)` copies it without the element at
  `at` (`rt_remove_seq`), answering that element. Positions follow the
  interpreter: `insert` takes `0..=len` (the length appends), `remove` and
  `pop` need an element that exists.
- `sort(xs)` is a stable insertion sort over a copy (`rt_sort_seq`). The
  analyzer's element shape picks the comparison -- ints, floats, or the
  runtime's `rt_cmp_str` for strings -- so only the element words move. Equal
  elements keep their order, as the interpreter's stable `sort_by` does. A
  list of any other element kind (bool, nested collection) is refused with
  `E177`: the interpreter orders those through its collection rules (`E042`),
  which would need the dynamic value word.
- A call carries no span, so the two run-time failures these builtins can hit
  are written without the interpreter's line clause, the same rule the
  arithmetic panics follow: `E110 pop() စာရင်းအလွတ်မှ ဖယ်၍မရပါ` for an empty
  pop and `E049 အညွှန်း <n> သည် အကွာအဝေးပြင်ပတွင် ရှိနေပါသည်။` for a position
  outside the list. Compile-time misuse keeps the interpreter's `E110` with its
  wording and without the line, and a `pop`/`remove` whose result is used while
  the element kind is still unknown is refused by the kind pass.
- `tests/native/fixtures/list_edits.akk` covers the returned values, inserting
  at the front, the middle and the length, a discarded result still changing
  the list, value semantics across a second binding, and int, float and string
  sorts, with and without `--no-opt`; `pop_empty` pins the empty-pop panic, and
  `insert_arity`/`sort_extra` pin `E110`. Parity moves to **10 match / 46
  refuse**: the corpus fixture `mutating` now compiles and matches the
  interpreter byte for byte.

### Step 11, switch lowering (landed)

`switch` (both spellings) now compiles, and it needed no runtime work at all:

- The AST arm became `HirStmt::Switch { subject, arms }`, which resolves the
  subject, every case expression and every arm body in the enclosing scope.
  The body of a `switch` is not a new scope in the interpreter, so a binding
  declared in an arm is visible after the switch, and HIR keeps that.
- MIR lowering reproduces the interpreter's evaluation order exactly: the
  subject goes into a fresh temporary once, then each `case` expression is
  evaluated and compared with `==` in written order until one matches. A case
  written after the matching one is never evaluated, so a call in it never
  runs. The first matching arm's block runs, the `default` arm's block is
  reached only after every case failed (wherever the default is written), and
  there is no fall-through. `==` is the language's own equality, so a float
  subject never equals an `int` case. All of this is `Compare`/`Branch`, the
  shape `if` already used, so the kind pass, the optimiser, codegen and the
  runtime are untouched.
- `break` needed one structural change: `Ctx` now keeps a `breaks` stack
  (switch ends and loop exits) apart from `loops` (continue targets). A
  `break` in an arm leaves through the switch's end block while a `continue`
  still goes to the enclosing loop, which is what the interpreter's
  `loop_depth` bump for a switch does. A `break` inside a switch with no loop
  around it therefore compiles, as it does when interpreted.
- `switch_forms` (the corpus fixture) now refuses later, at its `str(i)` call:
  the `str` conversion builtin is not in the slice yet, so its expectation
  keyword moved from `E176 switch` to `E177 a function call to "str"` rather
  than the count changing (10 match / 46 refuse). The native fixture
  `switch.akk` is the measured evidence for the construct; it is compared with
  the interpreter under both optimiser modes.
- Still refused by name: a `break` that carries a value in an arm, and a
  `continue` in a switch that is not inside a loop.

The interpreter and VM still produce byte-for-byte identical output and error
text, which the parity harness enforces.

### Step 12, string conversion and interpolation (landed)

`str(x)` and `{name}` interpolation now compile, and both are one conversion:

- The conversion builtins answer to `str`, `စာ` and `စာသား`, the names the
  interpreter's `convert_target` already accepts, and become `Builtin::Str`.
  The result kind is always `Str`, so the analyzer settles the destination
  without knowing the argument, and it refuses a collection argument (whose
  display form is the interpreter's `repr`, which would have to be rendered
  into a buffer) instead of guessing.
- Interpolation is a lowering rule, not a new runtime feature. `text_literal`
  reads the literal exactly as `interpolate_text` does -- `\{` and `\}` are
  literal braces, a `{…}` name is trimmed, placeholders are taken in source
  order -- and builds `str(name)` pieces joined with `+`. `+` on two strings is
  already `rt_concat_str`, so `"v = {x}"` and `"v = " + str(x)` are the same
  program. An unclosed brace, an empty `{}`, a `{အတု …}` field placeholder and
  an unknown name are refused by name (`E177`) rather than compiled.
- A placeholder becomes `MirInst::ToStr` rather than a `Call` to `str`, so a
  user function named `str` cannot shadow what interpolation means. The
  instruction is typed `Str`, carried by the DCE and constant-fold passes, and
  emitted by one codegen helper.
- The runtime gained `rt_str_int` (decimal digits built backwards on the stack,
  then copied into an arena `{ length, bytes }`), `rt_str_bool` (the interned
  `True`/`False` descriptors the printer uses, so `str(b)` and `print(b)`
  cannot disagree) and `rt_str_float`. The float formatter was already exact,
  so it is reused: a `.data` flag tells its final step to copy the rendered
  bytes into a fresh arena string instead of calling `rt_write`. The flag is
  cleared before returning, so printing is unaffected.
- Measured evidence: `tests/native/fixtures/stringify.akk` is compared with the
  interpreter under both optimiser modes, covering ints (positive, negative,
  zero and the largest), floats (fractional, whole, `-0.0`, a small and a very
  large one), bools, a Myanmar string, escaped braces, a concatenated
  conversion, and a parameter filled by a string and by an int call site.
  Parity moved to **13 match / 43 refuse**: `native_interpolation`,
  `eng_print` and `switch_forms` (which had been waiting on `str(i)`) compile
  and match byte for byte. `functions`, `eng_fn` and `collections` now refuse
  later than the interpolation, at their own unrelated gaps (kind inference for
  an unconstrained parameter, a function returning on only one path, and a
  binding re-used as a float and then a string).
- Still refused by name: `str()` of a collection, a `{အတု …}` placeholder, an
  unclosed or empty placeholder, and an interpolated name that is not in scope.

### Step 13, catchable arithmetic errors (landed)

A zero divisor for `/` or `%` now unwinds instead of exiting, so the
interpreter's arithmetic codes behave like the raises they always were:

- `HirExpr::Binary` carries the operator's line, which was previously dropped
  in lowering (the AST already had it). Lowering emits `MirInst::GuardZero`
  before every `/` and `%`: it names both operands, the operator, the line and
  the block the error goes to -- `raise_target`, i.e. the innermost enclosing
  `try` or the function's escape block. Being a real instruction with a
  successor, the guard survives DCE (its target counts as reachable) and its
  target is remapped when blocks are renumbered, so an optimised program cannot
  lose the edge.
- Codegen picks the code from the operand kinds, exactly as the interpreter
  does: two ints give `E036` for `/` and `E038` for `%`, any float operand
  gives `E037`/`E039`. A float divisor is compared with `ucomisd` and the
  unordered (NaN) case is skipped, so a `NaN` divisor is not a zero. The
  message is the interpreter's, built with the operator's line; the identity
  slot gets the code and the prefix slot is emptied, because a panic is not a
  named raise.
- The unwinding protocol therefore changed from "prefix means pending" to "the
  identity slot means pending": `check_error` after a call tests the identity,
  `clear_error` empties it, and a `bind_error` whose prefix is empty binds the
  code instead of a message. `E178` now tests that a prefix exists, so it takes
  a raise and not a panic -- the interpreter's `error_matches` rule -- while `E`
  still takes both. `rt_report_error` skips an empty slot, which is how an
  uncaught panic prints just its message.
- The analyzer accepts a clause for an interpreter code only when the program
  can produce it: `GuardZero` with `/` puts `E036` and `E037` in the raised set,
  `%` puts `E038` and `E039`. A clause for any other code (`E104`, `E049`) is
  still refused with `E177`.
- Measured evidence: the corpus fixtures `div_zero` and `mod_zero` moved from
  refused to **match**, byte for byte (stdout, stderr with line numbers, exit
  status), taking parity to **15 match / 41 refuse**. The new
  `tests/native/fixtures/arith_catch.akk` pins the interesting cases under both
  optimiser modes: `E` binds `E036`, `E037`/`E038`/`E039` are selected by code,
  a matching clause runs its `finally`, `E178` lets a panic through, and the
  final uncaught `%` prints `E038 လိုင်း 37 တွင် ...` and exits 1. The
  pre-existing `E037`/`E038`/`E039` panic tests now compare stderr exactly
  instead of only its first four characters.
- Unchanged: a compound remainder assignment (`x %= 0`) still calls
  `rt_panic_zero` and exits with `E038`, because `StoreVar` carries no line and
  has no guard in front of it; the unused `rt_panic_div_int`/
  `rt_panic_div_float` routines were removed.

### Step 14, `sum` and `keys` (landed)

Two reducers the analyzer can type without guessing now compile:

- `sum(xs)` walks the descriptor in generated code: `r10` is the base, `r11`
  the length, `rcx` the index, `xmm0` the running total, and each element word
  at `base + 24 + 16 * i` is promoted (`cvtsi2sd` for an int list, `movq` for a
  float one) and added. That is the interpreter's `f64` accumulation, so the
  result of an int list is the same after its saturating `as i64` cast --
  `f2i_*` labels reuse the conversion `floor`/`ceil`/`round` already needed
  (NaN is 0, at or past 2^63 is `i64::MAX`, below -2^63 is `i64::MIN`). The
  element shape the analyzer tracks chooses int or float; a collection of
  another kind, or one whose shape never settled, is refused with `E177`
  instead of compiling an `E110` the interpreter would raise at run time.
- `keys(d)` is `rt_items_dict`, the list a `for` loop over a dict already
  builds, so it adds no runtime code at all. The analyzer types the result as
  `list` of `str`, which is what makes `print`, `len`, indexing and iteration
  over it work; a non-dict argument is refused with `E177`.
- Measured evidence: `tests/native/fixtures/reducers.akk` compares both
  optimiser modes with the interpreter for an int list (`6`), a float list
  (`3.75`), a tuple (`9`), a set (`15`), a literal (`60`), an int list that
  saturates to `9223372036854775807`, two parameter call sites (int and float),
  and `keys` printed (`["a", "b"]`), measured with `len`, indexed (`b`) and
  walked by `for`. `sum` of a string list and `keys` of a list are pinned as
  refusals in `tests/native/main.rs`.
- Parity is unchanged at 15 match / 41 refuse: none of the refused corpus
  fixtures waits on these two builtins.

### Step 15, dynamic element shapes (landed)

Heterogeneous collections compile, without moving the value word yet:

- The runtime descriptor has always carried a tag beside every element word
  (`{ kind, len, [tag, word] * len }`), and the literal builder and the
  indexed write have always written the *value's own* tag. The one thing that
  refused a mixed collection was the analyzer's rule that a sequence literal's
  elements share one shape, so this slice is mostly that rule.
- `Kind` grew `Dynamic`, which is only ever a member of an element shape, never
  the kind of a value: it means "this collection's elements are not all one
  kind". `widen_shape` merges two shapes into one that describes both, marking
  the first disagreement `Dynamic` and dropping anything deeper, which is what
  the runtime can still say about the elements: `[Int]` beside `[Str]` is
  `[Dynamic]`, and `[List, Int]` beside `[List, Str]` is `[List, Dynamic]`. A
  shape that is a prefix of the other (`[]` beside `[1]`) keeps the longer one,
  because a collection that started empty has simply not settled yet.
- A sequence literal with mixed element kinds types as `Dynamic`. A store
  (`xs[0] = v`), `push`/`append` and `insert` widen the target's element shape
  the same way instead of refusing, so `mixed_store`, `push_mixed`,
  `nested_store_kind` and `nested_unknown_inner` all compile now; so does a
  binding re-assigned a collection of another element shape.
- Two literal paths needed runtime work, because their duplicates are only
  knowable when the program runs: a set member that is not a literal, and a
  dict key that is not a string literal. The obvious route -- appending each
  member through the copying indexed write (`rt_store_set`/`rt_store_dict`,
  one new descriptor and one full copy of the collection so far per member) --
  allocated once per member, so a literal rebuilt in a loop grew the arena in
  proportion to the collection's size on every iteration and could exhaust it
  even when the collection itself stayed tiny. Instead the code generator
  allocates the literal's final capacity once (`16 + 16 * n`, or
  `16 + 32 * n` for a dict) and the new `rt_fill_set`/`rt_fill_dict` helpers
  fill that descriptor in place: they deduplicate a set member, and give a
  repeated dict key its first position and last value. The rules are the
  interpreter's, unchanged; only the allocation count changed, from one per
  member to one per literal evaluation. A literal whose members are all
  constants keeps the compile-time deduplication and the direct descriptor
  build.
- An element read out of a `Dynamic` collection has no static kind, so it is
  refused with `E177` by name (indexing, iteration, and `pop`/`remove`/`sort`/
  `sum`, which answer or consume one element). This is the honest half of the
  step: the one-word NaN-boxed value from the top of this ADR is what would let
  such a read carry its tag in a temporary and dispatch at each use, and that
  is the next slice. Until then no code is emitted that would print or compare
  an element under a guessed kind.
- `tests/native/fixtures/dynamic_collections.akk` compares both optimiser modes
  with the interpreter for a mixed literal (print/`len`/equality), a store and
  a `push` that widen, a nested widening, a set with a run-time member and   a dict with a run-time key; the `dynamic_read`, `dynamic_iteration` and
  `dynamic_pop` cases in `tests/native/main.rs` pin the refusals, and
  `a_runtime_member_literal_allocates_once_per_evaluation` rebuilds both literal
  forms thousands of times so that a regression to per-member allocation would
  exhaust the arena and fail. Parity stays 15 match / 41 refuse: no corpus
  fixture is heterogeneous.

### Step 16, dict views (landed)

`values(d)` and `entries(d)` compile beside `keys(d)`; the value word stays as
it is, because the dict descriptor already carries a tag beside every key and
every value:

- `rt_values_dict` copies each pair's value word with its stored tag into a
  fresh list, and `rt_entries_dict` builds one two-element tuple per pair (the
  key is always a string, the value keeps the pair's tag) and puts those in a
  list. Both walk the pairs in insertion order, which is the order the
  descriptor stores and the interpreter's `IndexMap` iteration yields. One
  allocation pays for the list and, for `entries`, one more per tuple.
- The result types as a list: `values` takes the dict's value shape as its
  element shape, so a dict of lists stays readable at the second level, and
  `entries` takes `[Tuple, ...]` where the tuple contents are the widening of
  `[Str]` (the key) with the value shape. That widening is the same rule a
  written tuple of those kinds gets, so a string-valued dict leaves an entry
  position readable while a key beside a value of another kind settles the
  entry as `Dynamic`, whose inner read is refused like any dynamic read. No
  new refusal wording is needed beyond the existing dynamic-read message.
- A non-dict argument and a wrong arity reproduce the interpreter's `E110`
  (the same sentence `keys` already used), without the line clause compiled
  code cannot carry. `tests/native/fixtures/dict_views.akk` compares both
  optimiser modes with the interpreter for both views printed, counted,
  indexed and iterated, a string-valued dict's entry position and a
  list-valued dict's nested shape; the `values_of_list` and `entries_arity`
  cases pin the `E110` refusals. Parity is unchanged: no corpus fixture calls
  the two builtins.

### Step 17, dynamic element reads (landed)

A read out of a heterogeneous collection now carries its tag, without moving
the value word:

- The descriptor already stores every element as `{ tag, word }`, so the
  temporary of a dynamic read holds the **address of that pair** rather than a
  copy of either half. `rt_index_seq_ref` and `rt_index_dict_ref` return the
  pair address after the same bounds/key checks the ordinary reads do (the
  sequence pair starts at `16 + 16 * i`, a dict value's at `32 + 32 * i`), and
  `rt_index_seq`/`rt_index_dict` are now thin wrappers that load the word at
  `+8`. The address stays valid because the runtime copies collections on
  every write and never frees the arena, so the pair a temporary points at is
  never overwritten.
- The analyzer types such a read `Dynamic` (it no longer refuses it). Every
  use that needs one static kind refuses by name -- `str`, arithmetic,
  comparison, a condition, `len`, indexing the value itself, iterating the
  collection, `pop`/`remove`/`sort`/`sum` -- so no code is emitted that would
  treat the pair pointer as an int or a string.
- Printing dispatches on the stored tag: the code generator loads the pair's
  tag into `rdi` and its word into `rax` and calls `rt_print_dynamic`, which
  picks the same writer the static print uses (a string raw, not quoted) and
  ends with the statement newline; collection tags go through
  `rt_print_collection` and take the newline after their `repr`. Inside a
  collection literal the tag and word are copied into the new element
  (`Gen::load_tagged`), so `[xs[1], xs[0]]`, a set member and a dict value keep
  the kinds they were read with. A value also crosses a call unchanged: the
  pair address moves through the argument/return slots, and the analyzer types
  the parameter or result `Dynamic` from its call sites.
- `tests/native/fixtures/dynamic_reads.akk` compares both optimiser modes with
  the interpreter for int, string, float and bool reads, a read kept in a
  binding and copied into a new list, a collection-valued read, a parameter, a
  nested read beside a static one, a dict of mixed values (directly and through
  `values`/`entries`) and a set of mixed members. `dynamic_str` and
  `dynamic_arithmetic` pin the named refusals. Parity is unchanged: no corpus
  fixture waits on a heterogeneous read.

### Step 18, `slice` (landed)

`slice(xs, start[, end])` compiles over a list, tuple or set, still without
moving the value word:

- The runtime copies the selected elements' `{ tag, word }` pairs into a fresh
  descriptor through `rt_slice_seq`, so a heterogeneous collection slices and
  the result keeps the elements' own kinds (and their nested shapes, because
  an element that is itself a descriptor is copied by address). The result is
  always a list, which is what the interpreter answers for every sequence
  source: a tuple and a set slice into a list too.
- The interpreter's bounds rules are kept in one place: both ends clamp to the
  length, a start past the end is an empty result (not an error), and a start
  past the clamped end is ordered before the copy (`start = min(start, end)`).
  Only a negative start or end fails, which is the interpreter's `E110`; the
  executable writes that sentence with both values and no line clause, because
  a builtin call carries no span.
- The analyzer types the result as a list with the source's element shape and
  requires both positions to be ints (a `Dynamic` element shape is fine -- the
  copy carries tags). A dict is not a sequence, so it gets the interpreter's
  `E110`; text slices by grapheme cluster, which needs the Unicode tables, so
  it is refused with `E177` by name; a wrong arity is the interpreter's `E110`
  wording.
- `tests/native/fixtures/slice.akk` compares both optimiser modes with the
  interpreter for both bounds, the omitted end, clamping past the length, a
  start past the end, an empty result from reversed bounds, value semantics
  (the copy is fresh), a tuple, a set, nested lists and a run-time position;
  `slice_arity` and `slice_of_dict` pin the `E110` refusals, `string_slice`
  pins the `E177`, and
  `a_slice_with_a_negative_bound_panics_like_the_interpreter` checks the
  panic's exact sentence. Parity is unchanged: no corpus fixture calls
  `slice`.

### Step 19, `contains` (landed)

`contains(hay, needle)` compiles over a collection and over text, and the
value word is still where it was:

- A list, tuple or set answers whether one of its elements equals the needle;
  a dict asks its keys, which is the interpreter's `contains_key`. The scan
  (`rt_contains_seq`, `rt_contains_dict`) runs every stored `{ tag, word }`
  pair through `rt_value_eq`, so a heterogeneous collection answers by each
  element's own tag -- `contains([1, "a"], "a")` is `True` while
  `contains([1], True)` is `False`.
- Text is a byte scan (`rt_contains_str`), which is exactly what the
  interpreter's `str::contains` does: UTF-8 bytes, never characters, so a
  Myanmar substring is found wherever its bytes appear, an empty needle is
  found at the start, and a needle longer than the text is never found. No
  Unicode table is needed, which is why text is supported here while text
  `reverse`, `slice`, `len` and indexing are still refused.
- The step also fixed the equality the scan depends on. `rt_value_eq` took the
  left tag only, so `Int 0` and `Bool False` (and `Float 0.0` against `Int 0`)
  compared equal whenever their words agreed; the interpreter compares printed
  forms, which carry the type. The helper now takes the right tag in `rsi`,
  compares the two tags first, and every call site -- collection equality, the
  run-time set member and the set write -- passes it. `[0] == [False]` and
  `[0.0] == [0]` answer `False` again, and a set built at run time keeps both
  `0` and `False`.
- The analyzer types the answer `Bool`. For text the needle must settle to a
  string; for a collection any needle kind fits, because the comparison is the
  runtime's. A value whose kind is only known at run time (a dynamic read)
  cannot be told apart as text from a collection, so it is refused with `E177`
  by name rather than lowered under a guessed rule. A wrong arity, a first
  argument that is neither text nor a collection, and a text search for a
  non-text value keep the interpreter's `E110` wording, without the line
  clause compiled code has no span for.
- `tests/native/fixtures/contains.akk` compares both optimiser modes with the
  interpreter for a list, a tuple, a set, a dict (its keys, and a value that
  is not a key), nested collections, mixed elements (`1`, `2.5`, `True`, with
  `False` and `1.0` as non-matches), text (a Myanmar substring, empty, longer,
  a suffix, absent), a run-time needle, a parameter, and the tag rule through
  `[0] == [False]`, `[0.0] == [0]` and a run-time set; `contains_arity`,
  `contains_of_int` and `contains_text_needle` pin the `E110` refusals, and
  `contains_dynamic_value`/`contains_dynamic_needle` the `E177` ones. Parity is
  unchanged: no corpus fixture calls `contains`.

### Step 20, `join` and `split` (landed)

`join(xs, sep)` and `split(text, sep)` compile, and the value word is still
where it was. Neither needs a new value kind: `join` reads the descriptor a
collection already has, and `split` answers an ordinary list of strings.

- `join` walks a list, tuple or set descriptor and spells each element by its
  stored tag -- `rt_str_int`, `rt_str_float`, `rt_str_bool`, or a string's own
  bytes -- concatenating the pieces with `rt_concat_str` into one fresh arena
  string. That is `print`'s spelling, so `join([1, 2.5, True], "-")` is
  `1-2.5-True` and a Myanmar element keeps its bytes. The tag dispatch is what
  lets a heterogeneous collection join, and an empty collection answers the
  empty string. An element that is itself a collection would need the
  interpreter's nested rendering (`[1, 2]`, strings quoted), which this runtime
  does not build into text yet: a collection whose static element kind is a
  collection is refused with `E177`, and a heterogeneous collection that hands
  the runtime one anyway writes a documented `E177` line and exits 1 instead of
  a wrong spelling.
- `split` is a byte scan, exactly the interpreter's `str::split`: it counts the
  non-overlapping occurrences first, allocates the list at its exact size, then
  copies one string descriptor per piece. `"a,,b"` keeps its empty middle
  piece, `"abc"` answers one piece, empty text answers one empty piece, and a
  Myanmar separator matches wherever its bytes appear.
- An empty separator asks for grapheme clusters -- one character per piece, so
  a syllable like `မာ` is never cut in half -- which needs the syllable table
  this runtime does not carry. A literal empty separator is refused at build
  time with `E177`; one that only turns out empty at run time (a parameter, say)
  writes a documented `E177` line and exits 1. Splitting by scalar value
  instead would be silently different from the interpreter, which is the one
  thing this backend never does.
- The interpreter's `E110` wording is kept for both: its two-argument
  sentence, its separator-first check and its "first argument is a collection"
  sentence for `join` (a dict included), and its single "text and separator"
  sentence for `split`, all without the line clause a call cannot carry.
- `tests/native/fixtures/join_split.akk` compares both optimiser modes with the
  interpreter for a list, a tuple, a set, an empty list, int/float/bool
  elements, a separator through a binding and through a parameter, and for
  `split` with empty pieces, one piece, empty text, repeated spaces, a Myanmar
  separator and a two-byte separator; `join_arity`, `join_of_int`,
  `join_of_dict` and `split_of_int` pin the `E110` refusals,
  `join_of_collections` and `split_empty_separator` the build-time `E177` ones,
  and `native_only_limits_end_with_a_documented_e177` the two that are only
  visible at run time. Parity is unchanged: no corpus fixture calls `join` or
  `split`.

### Step 21, `starts_with` and `ends_with` (landed)

`starts_with(text, part)` and `ends_with(text, part)` compile, and nothing about
the value word changes -- both answer a `Bool` from bytes that are already in
the constant pool or the arena.

- A prefix compares the part's bytes with the text's from offset 0; a suffix
  compares them from `len(text) - len(part)`. That is the interpreter's
  `str::starts_with`/`str::ends_with` over UTF-8, so a part longer than the
  text never matches and an empty part always does. The two routines
  (`rt_starts_str`, `rt_ends_str`) differ only in how that offset is computed.
- No grapheme table is involved: a byte prefix of a text is still text, so
  `starts_with("မြန်မာ", "မြ")` is `True` and `ends_with("မြန်မာ", "မာ")` is
  `True`, while a part that is not at that end (`"န်မာ"`, `"မ"`) is a plain
  `False` -- the interpreter answers the same, because a match has to start at
  a boundary the text actually has.
- The interpreter's `E110` wording is kept: its two-argument sentence, and its
  single "text and part" sentence when either side is not text. A part whose
  kind is only known at run time is refused with `E177` by name.
- `tests/native/fixtures/starts_ends.akk` compares both optimiser modes with
  the interpreter for a prefix, a suffix, an empty part, a part longer than the
  text, empty text, Myanmar boundaries, a binding, a parameter and a condition;
  `starts_with_arity`, `starts_with_of_int` and `ends_with_of_int` pin the
  `E110` refusals, and `starts_with_dynamic_part` the `E177` one. Parity is
  unchanged: no corpus fixture calls either builtin.

### Step 22, `replace` (landed)

`replace(text, from, to)` compiles, and nothing about the value word changes: it
answers one fresh string in the arena, exactly like `join`.

- The runtime counts the non-overlapping occurrences of `from` first, allocates
  the result at its exact size (`len(text) - count * len(from) + count *
  len(to)`) and then copies the text around each replacement through
  `rt_replace_str`. That is the interpreter's `str::replace` over bytes, so a
  Myanmar piece is replaced wherever its bytes appear -- and no grapheme table
  is needed, because a replacement never has to find a cluster boundary. Setting
  `to` empty deletes, and a `from` that is absent leaves the text alone.
- An empty `from` answers the text unchanged, which is the interpreter's own
  rule rather than inserting `to` between every character. That is why this
  step needs no refusal at all: every shape of the interpreter's `replace` is
  emittable.
- The interpreter's `E110` wording is kept: its three-argument sentence, and
  its single "three texts" sentence when any argument is not text. A piece
  whose kind is only known at run time is refused with `E177` by name.
- `tests/native/fixtures/replace.akk` compares both optimiser modes with the
  interpreter for repeated pieces, a piece that is absent, deletion, an empty
  `from`, empty text, Myanmar pieces (a whole syllable and one inside a word), a
  binding and a parameter; `replace_arity` and `replace_of_int` pin the `E110`
  refusals, and `replace_dynamic_from` the `E177` one. Parity is unchanged: no
  corpus fixture calls `replace`.

### Step 23, `enumerate` (landed)

`enumerate(xs)` compiles for a list, tuple or set, and the value word is still
where it was: each pair is a descriptor of its own, exactly the shape a tuple
literal's descriptor has.

- `rt_enumerate_seq` allocates the result list at the source's length, then for
  each element allocates a two-element tuple `{ TAG_TUPLE, 2, { TAG_INT, i },
  { tag, word } }`, copying the element's stored tag and word. That is the
  interpreter's `enumerate`, so a heterogeneous collection enumerates and the
  pair prints as `(0, "a")`.
- The result shape is `[Tuple] + widen([Int], elements)`. A tuple's shape was
  homogeneous at this point, so an int-valued collection kept both halves
  readable (`pair[0]`, `pair[1]`), while a value of another kind widened the
  pair to `Dynamic`; Step 24 replaced that with one shape per position. Either
  way printing, `len` and iteration over the pairs work.
- `enumerate` of a string counts grapheme clusters, so it is refused with
  `E177` by name (the reason `len`/index/`slice`/`split(s, "")` are); a dict is
  not a sequence and keeps the interpreter's `E110`; a value whose kind is only
  known at run time is refused with `E177`.
- `tests/native/fixtures/enumerate.akk` compares both optimiser modes with the
  interpreter for a list of ints (printed, counted, a pair read by index), a
  list of strings (printed and its first pair read), a loop over the pairs, a
  tuple, a set and an empty list; `enumerate_arity`, `enumerate_of_dict` and
  `enumerate_of_int` pin the `E110` refusals, and `enumerate_of_string` and
  `enumerate_dynamic` the `E177` ones. Parity is unchanged: no corpus fixture
  calls `enumerate`.

### Step 24, positional tuple shapes (landed)

A tuple no longer has to hold one kind all the way through: the analyzer's
shape is a tree, and a tuple keeps one shape per position.

- `Shape` replaces the flat `Vec<Kind>`: `Scalar(kind)`, `Seq(kind, inner)` for
  a list or set, `Dict(inner)`, `Tuple(positions)`, plus `Unsettled` (nothing
  known yet -- an empty literal) and `Mixed` (more than one kind, decided at
  run time). `elems`/`var_elems`, a parameter's learned shape and a function's
  result shape all hold one. `widen_shape` marks a disagreement `Mixed`,
  `merge_shapes` still lets one side say less, and `Shape::refines` is what
  lets a later fixpoint round tighten an unsettled shape into a settled one
  instead of contradicting it.
- `learn_indexed` now takes the index operand: a constant position picks that
  position's shape (`pair[0]` the int, `pair[1]` the string), any other index
  answers what all the positions agree on, and a `Mixed` position is read as
  `Dynamic` -- the code generator leaves the element's own tag beside its word,
  so printing it works while a use that needs one kind is refused. An
  `Unsettled` shape says nothing and so learns nothing, which is what keeps an
  empty string's `for` loop from typing its variable as dynamic.
- A store into a tuple updates the position it names (`Shape::stored`), so
  `pair[1] = "z"` leaves `pair[0]` readable; storing another kind there makes
  that position `Mixed` rather than silently a different kind.
- `enumerate(xs)`'s pair is `Tuple([Int, element])` and `entries(d)`'s is
  `Tuple([Str, value])`, so both positions are readable even when the source
  mixes kinds -- `enumerate([1, "a"])[0][0]` is the int 0. `reverse` reverses a
  tuple's positions along with its elements, so `reverse(pair)[0]` is the
  pair's last position.
- The runtime is untouched: the descriptor already carries a tag beside every
  element word, so a positionally typed read is the same word read.
- `tests/native/fixtures/positional.akk` compares both optimiser modes with the
  interpreter for a mixed tuple literal, `enumerate` and `entries` pairs read
  at both positions, a reversed pair, a nested list, and a pair out of a mixed
  list. `strings_ops` (an empty string's loop) guards the `Unsettled` rule. The
  corpus parity counts are unchanged.

### Step 25, square root (landed)

`sqrt(x)` was the last numeric builtin the slice listed as outside it, and the
only reason was its `E111`: that sentence quotes the offending value.

- The analyzer answers `Float` whatever the argument is (`sqrt(16)` is `4.0`,
  not `4`), and refuses a non-number with the interpreter's `E110` before any
  code is emitted.
- The generator converts an int to a double and takes one `sqrtsd`, which is
  what `f64::sqrt` compiles to, so every result is bit-for-bit the
  interpreter's. `-0.0` keeps its sign; a NaN is unordered under `ucomisd`,
  which takes neither branch of the sign test, and its root is a NaN.
- The quoted value is what blocked this. `rt_sqrt_neg_int` writes the fixed
  prefix, the int through `rt_write_int_err`, and the fixed suffix, then exits
  1; `rt_sqrt_neg_float` formats the double with `rt_str_float` -- the same
  formatter `print` uses, so the digits cannot drift -- and writes that string.
  The line clause is omitted, the rule every run-time panic in compiled code
  follows.
- `tests/native/fixtures/sqrt.akk` compares both optimiser modes with the
  interpreter, and the `E111` path is pinned for an int, a float and a
  parameter. The corpus parity counts are unchanged.

### Step 26, trim (landed)

`trim(text)` is the first text builtin the slice takes on that needs Unicode
rules. `find`/`index_of` were considered first and set aside: their answer is a
grapheme-cluster index, which needs the syllable table the runtime does not
carry. `trim` needs only the set of scalars `char::is_whitespace` names.

- That set is about two dozen scalars, so `rt_ws_len` is emitted as byte
  comparisons generated from the host's own `char::is_whitespace`. Matching
  the bytes of a scalar is the same test `str::trim` makes, so a no-break
  space or an ideographic space is trimmed exactly where the interpreter trims
  it, and nothing has to be trusted from memory.
- `rt_trim_str` walks forwards one scalar at a time and backwards to the lead
  byte (`< 0xC0` is not a continuation byte), dropping a scalar only when its
  own bytes end exactly at the current hi. A multi-byte scalar is never split,
  so a Myanmar syllable survives or goes whole, and the bytes between the two
  ends are copied unchanged with `rep movsb`.
- A text that needs no trimming answers itself: strings are immutable in the
  generated code, so sharing the descriptor is exact and allocates nothing.
- `tests/native/fixtures/trim.akk` compares both optimiser modes with the
  interpreter, including a no-break space and an ideographic space, and
  `trim_arity`/`trim_of_int` pin the `E110` refusals.

A single NaN-boxed word is still the longer-term shape (it would also make
`str`/arithmetic on a dynamic value dispatchable), but the pair address needs
no change to the element storage, the ABI or the static kinds every other
instruction depends on.
