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
- `len(collection)` and `MirInst::Len` read the descriptor's length; the
  `for`-loop `Len`/`Index` shape is emitted, but `MirInst::Items` (iteration)
  is still refused, so no parity fixture flips: the split remains 6 match /
  48 refuse, with the refusal for each collection fixture now naming the next
  construct instead of "a list".
- New native fixture `tests/native/fixtures/collections.akk` is compared with
  the interpreter through `akk build` in both optimiser modes. Two native
  tests cover the rest: `collection_diagnostics_match_the_interpreter` asserts
  byte-identical `E049`/`E053` stderr for range, negative and missing-key
  cases, and `the_collection_arena_reports_exhaustion_honestly` observes the
  documented exhaustion path. Three unit tests cover index typing and the
  heterogeneous-list / run-time-set refusals.

Still deferred (and to which step):

- **Step 3:** `Items`/`for`-over-collection, `StoreIndex`/`StoreField`,
  collection equality/ordering, indexing a nested collection more than one
  level deep or a collection parameter (element kinds do not cross calls yet),
  `len(string)` (grapheme clusters), run-time set members and dict keys,
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

The interpreter and VM still produce byte-for-byte identical output and error
text, which the parity harness enforces.
