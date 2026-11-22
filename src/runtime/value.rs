//! The runtime value word: 64-bit NaN-boxing and its value-level operations
//! (Step 2).
//!
//! This module is scaffolding: no engine uses it yet, so a program's output is
//! unchanged. It fixes the representation the native runtime will use and
//! proves, by test, that every Akkhara value kind survives an encode/decode
//! round trip, that heap payloads are really header-backed, and that display,
//! equality and condition rules match the interpreter's.
//!
//! A value is one 64-bit word. Ordinary IEEE-754 doubles use the whole word.
//! Every other kind lives in the quiet-NaN space that no real double can use:
//!
//! ```text
//!  63 62..52 51 50..47 46..0
//!  1  111...1 1  tag   47-bit payload
//! ```
//!
//! Floats are canonicalised: every NaN is stored as [`CANONICAL_NAN`], so no
//! double bit pattern can collide with a tagged word. The payload of a heap
//! value is its address, which must fit in 47 bits and be 8-byte aligned.
//! `int` is heap-boxed because a full `i64` does not fit in 47 bits; the ADR
//! (`docs/adr/0001-runtime-value-representation.md`) explains that trade-off.
//!
//! Heap payloads start with an `alloc::Header` and are allocated by
//! `alloc::Heap`; this module adds the typed payloads, constructors, accessors
//! and the value-level dispatch (display/repr, equality, condition).

#![allow(dead_code)] // scaffolding: the engines do not use it yet

use std::cmp::Ordering;
use std::fmt::Write as _;

use crate::runtime::alloc::{Header, Heap, HeapKind, HeapPayload};

/// The one NaN bit pattern a float may keep. Every other NaN is folded here.
pub(crate) const CANONICAL_NAN: u64 = 0x7FF8_0000_0000_0000;

/// Bits 63..51: sign 1, exponent all ones, quiet bit 1. A tagged word always
/// has these bits set; a canonical NaN has the sign bit clear instead.
const TAG_MASK: u64 = 0xFFF8_0000_0000_0000;
/// Bits 46..0: an 8-byte-aligned pointer, or zero for the tag-only kinds.
const PAYLOAD_MASK: u64 = (1 << 47) - 1;
const TAG_SHIFT: u32 = 47;

pub(crate) const TAG_NULL: u64 = 0;
pub(crate) const TAG_FALSE: u64 = 1;
pub(crate) const TAG_TRUE: u64 = 2;
pub(crate) const TAG_INT: u64 = 3;
pub(crate) const TAG_STR: u64 = 4;
pub(crate) const TAG_LIST: u64 = 5;
pub(crate) const TAG_TUPLE: u64 = 6;
pub(crate) const TAG_SET: u64 = 7;
pub(crate) const TAG_DICT: u64 = 8;
pub(crate) const TAG_OBJECT: u64 = 9;
pub(crate) const TAG_FUNCTION: u64 = 10;

/// A descriptor-only tag: a float occupies the untagged bit space in a
/// `Value`, so a runtime collection element needs an explicit tag to say
/// "these bits are a double" (the codegen bridge uses this vocabulary).
pub(crate) const TAG_FLOAT: u64 = 11;
/// A descriptor-only tag for a bool whose value decides the tag: a boxed
/// `Value` uses [`TAG_TRUE`]/[`TAG_FALSE`], but a collection element stores
/// the word `0`/`1` and inspects it at print time.
pub(crate) const TAG_BOOL: u64 = 12;

/// A boxed `int`: a full `i64` plus its GC header.
#[repr(C)]
pub(crate) struct HeapInt {
    pub(crate) header: Header,
    pub(crate) value: i64,
}

/// A string: UTF-8 bytes with the vector's length and capacity.
#[repr(C)]
pub(crate) struct HeapString {
    pub(crate) header: Header,
    pub(crate) bytes: Vec<u8>,
}

/// A list: an ordered vector of values.
#[repr(C)]
pub(crate) struct HeapList {
    pub(crate) header: Header,
    pub(crate) items: Vec<Value>,
}

/// A tuple: the same storage as a list, with its own header kind.
#[repr(C)]
pub(crate) struct HeapTuple {
    pub(crate) header: Header,
    pub(crate) items: Vec<Value>,
}

/// A set: insertion-ordered members. The production set adds a hash index.
#[repr(C)]
pub(crate) struct HeapSet {
    pub(crate) header: Header,
    pub(crate) items: Vec<Value>,
}

/// A dictionary. This spike keeps insertion-ordered pairs; the production
/// runtime adds a hash index over the same entries.
#[repr(C)]
pub(crate) struct HeapDict {
    pub(crate) header: Header,
    pub(crate) entries: Vec<(Value, Value)>,
}

/// A class instance: its class name and fields in declaration order.
#[repr(C)]
pub(crate) struct HeapObject {
    pub(crate) header: Header,
    pub(crate) class: String,
    pub(crate) fields: Vec<(String, Value)>,
}

/// A callable value. A closure's captured cells will extend this.
#[repr(C)]
pub(crate) struct HeapFunction {
    pub(crate) header: Header,
    pub(crate) name: String,
    pub(crate) arity: usize,
}

macro_rules! heap_payload {
    ($type:ty, $kind:expr) => {
        impl HeapPayload for $type {
            const KIND: HeapKind = $kind;

            fn header(&self) -> &Header {
                &self.header
            }
        }
    };
}

heap_payload!(HeapInt, HeapKind::Int);
heap_payload!(HeapString, HeapKind::String);
heap_payload!(HeapList, HeapKind::List);
heap_payload!(HeapTuple, HeapKind::Tuple);
heap_payload!(HeapSet, HeapKind::Set);
heap_payload!(HeapDict, HeapKind::Dict);
heap_payload!(HeapObject, HeapKind::Object);
heap_payload!(HeapFunction, HeapKind::Function);

/// One Akkhara value in one 64-bit word.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Value(u64);

impl Value {
    fn tagged(tag: u64, payload: u64) -> Value {
        debug_assert!(payload <= PAYLOAD_MASK, "payload exceeds 47 bits");
        debug_assert_eq!(payload & 0b111, 0, "heap pointers must be 8-byte aligned");
        Value(TAG_MASK | (tag << TAG_SHIFT) | payload)
    }

    fn from_pointer(tag: u64, pointer: *const u8) -> Value {
        Value::tagged(tag, pointer as u64)
    }

    pub(crate) fn null() -> Value {
        Value::tagged(TAG_NULL, 0)
    }

    pub(crate) fn boolean(value: bool) -> Value {
        Value::tagged(if value { TAG_TRUE } else { TAG_FALSE }, 0)
    }

    /// A float keeps its exact bits, except that every NaN becomes
    /// [`CANONICAL_NAN`] so it cannot be mistaken for a tag.
    pub(crate) fn float(value: f64) -> Value {
        if value.is_nan() {
            Value(CANONICAL_NAN)
        } else {
            let bits = value.to_bits();
            debug_assert_ne!(
                bits & TAG_MASK,
                TAG_MASK,
                "a real double cannot use the tag space"
            );
            Value(bits)
        }
    }

    /// The raw word, for tests and for the runtime helpers that will move it
    /// between stack slots and registers.
    pub(crate) fn bits(self) -> u64 {
        self.0
    }

    pub(crate) fn is_tagged(self) -> bool {
        self.0 & TAG_MASK == TAG_MASK
    }

    fn tag(self) -> Option<u64> {
        self.is_tagged().then(|| (self.0 >> TAG_SHIFT) & 0xF)
    }

    fn payload(self) -> u64 {
        self.0 & PAYLOAD_MASK
    }

    /// Dereference a heap payload after checking its tag and (in debug
    /// builds) its header kind.
    fn heap<T>(self, tag: u64, kind: HeapKind) -> Option<&'static T> {
        if self.tag()? != tag {
            return None;
        }
        let pointer = self.payload() as *const T;
        debug_assert_eq!(
            unsafe { (*(pointer as *const Header)).kind },
            kind,
            "the value tag and the heap header disagree"
        );
        Some(unsafe { &*pointer })
    }

    pub(crate) fn is_null(self) -> bool {
        self.tag() == Some(TAG_NULL)
    }

    pub(crate) fn as_bool(self) -> Option<bool> {
        match self.tag()? {
            TAG_FALSE => Some(false),
            TAG_TRUE => Some(true),
            _ => None,
        }
    }

    pub(crate) fn as_int(self) -> Option<i64> {
        self.heap::<HeapInt>(TAG_INT, HeapKind::Int)
            .map(|int| int.value)
    }

    pub(crate) fn as_float(self) -> Option<f64> {
        (!self.is_tagged()).then(|| f64::from_bits(self.0))
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        Value::heap::<HeapString>(*self, TAG_STR, HeapKind::String)
            .map(|text| std::str::from_utf8(&text.bytes).expect("runtime strings are always UTF-8"))
    }

    pub(crate) fn as_list(&self) -> Option<&'static [Value]> {
        Value::heap::<HeapList>(*self, TAG_LIST, HeapKind::List).map(|list| list.items.as_slice())
    }

    pub(crate) fn as_tuple(&self) -> Option<&'static [Value]> {
        Value::heap::<HeapTuple>(*self, TAG_TUPLE, HeapKind::Tuple)
            .map(|tuple| tuple.items.as_slice())
    }

    pub(crate) fn as_set(&self) -> Option<&'static [Value]> {
        Value::heap::<HeapSet>(*self, TAG_SET, HeapKind::Set).map(|set| set.items.as_slice())
    }

    pub(crate) fn as_dict(&self) -> Option<&'static [(Value, Value)]> {
        Value::heap::<HeapDict>(*self, TAG_DICT, HeapKind::Dict).map(|dict| dict.entries.as_slice())
    }

    pub(crate) fn as_object(&self) -> Option<&'static HeapObject> {
        Value::heap::<HeapObject>(*self, TAG_OBJECT, HeapKind::Object)
    }

    pub(crate) fn as_function(&self) -> Option<&'static HeapFunction> {
        Value::heap::<HeapFunction>(*self, TAG_FUNCTION, HeapKind::Function)
    }

    /// Only a real `bool` is a condition; the interpreter raises `E043` for
    /// every other kind rather than treating numbers or strings as truthy.
    pub(crate) fn condition(self) -> Option<bool> {
        self.as_bool()
    }

    /// The text `print` writes: strings raw, collections `repr`-style.
    pub(crate) fn display(self) -> String {
        let mut out = String::new();
        self.display_into(&mut out);
        out
    }

    fn display_into(self, out: &mut String) {
        if let Some(text) = self.as_str() {
            out.push_str(text);
        } else if let Some(int) = self.as_int() {
            let _ = write!(out, "{int}");
        } else if let Some(float) = self.as_float() {
            // The interpreter prints an integral float with one decimal place
            // (`1.0`), and every other float with Rust's shortest spelling.
            if float.fract() == 0.0 {
                let _ = write!(out, "{float:.1}");
            } else {
                let _ = write!(out, "{float}");
            }
        } else if let Some(flag) = self.as_bool() {
            out.push_str(if flag { "True" } else { "False" });
        } else if self.is_null() {
            out.push_str("null");
        } else {
            self.repr_into(out);
        }
    }

    /// The representation used inside collections (strings quoted) and for a
    /// collection printed at the top level.
    pub(crate) fn repr(self) -> String {
        let mut out = String::new();
        self.repr_into(&mut out);
        out
    }

    fn repr_into(self, out: &mut String) {
        if let Some(text) = self.as_str() {
            out.push('"');
            out.push_str(text);
            out.push('"');
        } else if let Some(items) = self.as_list() {
            repr_sequence(items, '[', ']', out);
        } else if let Some(items) = self.as_tuple() {
            repr_sequence(items, '(', ')', out);
        } else if let Some(items) = self.as_set() {
            repr_sequence(items, '{', '}', out);
        } else if let Some(entries) = self.as_dict() {
            out.push('{');
            for (index, (key, value)) in entries.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                key.repr_into(out);
                out.push_str(": ");
                value.repr_into(out);
            }
            out.push('}');
        } else if let Some(object) = self.as_object() {
            out.push_str(&object.class);
            out.push_str(" {");
            for (index, (field, value)) in object.fields.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                out.push_str(field);
                out.push_str(": ");
                value.repr_into(out);
            }
            out.push('}');
        } else if let Some(function) = self.as_function() {
            out.push_str(&function.name);
        } else {
            self.display_into(out);
        }
    }
}

fn repr_sequence(items: &[Value], open: char, close: char, out: &mut String) {
    out.push(open);
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        item.repr_into(out);
    }
    out.push(close);
}

/// Structural equality, matching the interpreter: the primitive same-kind
/// cases compare directly; everything else compares its `repr`. That keeps
/// `1` unequal to `1.0`, makes two `NaN`s equal (as the interpreter's
/// representation comparison does), and compares collections structurally.
pub(crate) fn value_eq(left: Value, right: Value) -> bool {
    if let (Some(left), Some(right)) = (left.as_int(), right.as_int()) {
        return left == right;
    }
    if let (Some(left), Some(right)) = (left.as_str(), right.as_str()) {
        return left == right;
    }
    if let (Some(left), Some(right)) = (left.as_bool(), right.as_bool()) {
        return left == right;
    }
    if left.is_null() && right.is_null() {
        return true;
    }
    left.repr() == right.repr()
}

impl Heap {
    /// Box an `int`. A full `i64` does not fit in 47 payload bits, so every
    /// integer is heap-backed until a small-int fast path is added.
    pub(crate) fn int(&mut self, value: i64) -> Value {
        let pointer = self.alloc_payload(HeapInt {
            header: Header::new(HeapKind::Int),
            value,
        });
        Value::from_pointer(TAG_INT, pointer as *const u8)
    }

    pub(crate) fn string(&mut self, text: impl Into<String>) -> Value {
        let pointer = self.alloc_payload(HeapString {
            header: Header::new(HeapKind::String),
            bytes: text.into().into_bytes(),
        });
        Value::from_pointer(TAG_STR, pointer as *const u8)
    }

    pub(crate) fn list(&mut self, items: Vec<Value>) -> Value {
        let pointer = self.alloc_payload(HeapList {
            header: Header::new(HeapKind::List),
            items,
        });
        Value::from_pointer(TAG_LIST, pointer as *const u8)
    }

    pub(crate) fn tuple(&mut self, items: Vec<Value>) -> Value {
        let pointer = self.alloc_payload(HeapTuple {
            header: Header::new(HeapKind::Tuple),
            items,
        });
        Value::from_pointer(TAG_TUPLE, pointer as *const u8)
    }

    pub(crate) fn set(&mut self, items: Vec<Value>) -> Value {
        let pointer = self.alloc_payload(HeapSet {
            header: Header::new(HeapKind::Set),
            items,
        });
        Value::from_pointer(TAG_SET, pointer as *const u8)
    }

    pub(crate) fn dict(&mut self, entries: Vec<(Value, Value)>) -> Value {
        let pointer = self.alloc_payload(HeapDict {
            header: Header::new(HeapKind::Dict),
            entries,
        });
        Value::from_pointer(TAG_DICT, pointer as *const u8)
    }

    pub(crate) fn object(
        &mut self,
        class: impl Into<String>,
        fields: Vec<(String, Value)>,
    ) -> Value {
        let pointer = self.alloc_payload(HeapObject {
            header: Header::new(HeapKind::Object),
            class: class.into(),
            fields,
        });
        Value::from_pointer(TAG_OBJECT, pointer as *const u8)
    }

    pub(crate) fn function(&mut self, name: impl Into<String>, arity: usize) -> Value {
        let pointer = self.alloc_payload(HeapFunction {
            header: Header::new(HeapKind::Function),
            name: name.into(),
            arity,
        });
        Value::from_pointer(TAG_FUNCTION, pointer as *const u8)
    }

    /// `left + right` for two strings, as the language's string `+` will need.
    pub(crate) fn concat_strings(&mut self, left: Value, right: Value) -> Option<Value> {
        let mut text = String::from(left.as_str()?);
        text.push_str(right.as_str()?);
        Some(self.string(text))
    }

    /// A `[start, end)` byte slice of a string, or `None` when the range is
    /// not on a character boundary.
    pub(crate) fn slice_string(&mut self, text: Value, start: usize, end: usize) -> Option<Value> {
        let slice = text.as_str()?.get(start..end)?;
        Some(self.string(slice))
    }

    /// Lexicographic string order, the rule `rt_cmp_str` implements in
    /// machine code for the current slice.
    pub(crate) fn compare_strings(left: Value, right: Value) -> Option<Ordering> {
        Some(left.as_str()?.cmp(right.as_str()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_round_trips_through_one_word() {
        let mut heap = Heap::new();

        assert!(Value::null().is_null());
        assert_eq!(Value::boolean(true).as_bool(), Some(true));
        assert_eq!(Value::boolean(false).as_bool(), Some(false));

        for int in [0, 1, -1, 42, i64::MIN, i64::MAX, 1 << 46, -(1 << 46)] {
            let value = heap.int(int);
            assert_eq!(value.as_int(), Some(int), "int {int}");
            assert!(value.is_tagged());
        }

        for float in [0.0, -0.0, 1.5, -2.25, f64::INFINITY, f64::NEG_INFINITY] {
            let value = Value::float(float);
            assert!(!value.is_tagged(), "a float must stay inline");
            assert_eq!(value.as_float(), Some(float), "float {float}");
        }
        let nan = Value::float(f64::NAN);
        assert!(nan.as_float().expect("a float").is_nan());
        assert_eq!(nan.bits(), CANONICAL_NAN);

        let text = heap.string("မင်္ဂလာပါ {x}");
        assert_eq!(text.as_str(), Some("မင်္ဂလာပါ {x}"));

        let one = heap.int(1);
        let two = heap.string("two");
        let list = heap.list(vec![one, two]);
        let items = list.as_list().expect("a list");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].as_int(), Some(1));
        assert_eq!(items[1].as_str(), Some("two"));

        let tuple = heap.tuple(vec![Value::boolean(true), Value::null()]);
        assert_eq!(tuple.as_tuple().expect("a tuple").len(), 2);

        let seven = heap.int(7);
        let set = heap.set(vec![seven]);
        assert_eq!(set.as_set().expect("a set").len(), 1);

        let key = heap.string("a");
        let one = heap.int(1);
        let dict = heap.dict(vec![(key, one)]);
        let pairs = dict.as_dict().expect("a dict");
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0.as_str(), Some("a"));
        assert_eq!(pairs[0].1.as_int(), Some(1));

        let age = heap.int(30);
        let object = heap.object("Person", vec![("age".to_string(), age)]);
        let object = object.as_object().expect("an object");
        assert_eq!(object.class, "Person");
        assert_eq!(object.fields[0].0, "age");
        assert_eq!(object.fields[0].1.as_int(), Some(30));

        let function = heap.function("twice", 1);
        let function = function.as_function().expect("a function");
        assert_eq!(function.name, "twice");
        assert_eq!(function.arity, 1);
    }

    #[test]
    fn the_word_is_eight_bytes_and_heap_payloads_are_aligned() {
        assert_eq!(std::mem::size_of::<Value>(), 8);
        assert_eq!(std::mem::align_of::<Value>(), 8);

        let mut heap = Heap::new();
        let heap_values = [
            heap.int(9),
            heap.string("x"),
            heap.list(Vec::new()),
            heap.tuple(Vec::new()),
            heap.set(Vec::new()),
            heap.dict(Vec::new()),
            heap.object("C", Vec::new()),
            heap.function("f", 0),
        ];
        for value in heap_values {
            assert!(value.is_tagged());
            assert_ne!(value.payload(), 0);
            assert_eq!(value.payload() & 0b111, 0, "8-byte alignment");
            assert!(value.payload() <= PAYLOAD_MASK);
        }
        assert_eq!(heap.object_count(), heap_values.len());
    }

    #[test]
    fn the_tags_are_distinct_and_outside_the_float_space() {
        let mut heap = Heap::new();
        let tags = [
            Value::null().tag(),
            Value::boolean(false).tag(),
            Value::boolean(true).tag(),
            heap.int(0).tag(),
            heap.string("").tag(),
            heap.list(Vec::new()).tag(),
            heap.tuple(Vec::new()).tag(),
            heap.set(Vec::new()).tag(),
            heap.dict(Vec::new()).tag(),
            heap.object("C", Vec::new()).tag(),
            heap.function("f", 0).tag(),
        ];
        let mut seen = std::collections::BTreeSet::new();
        for tag in tags {
            let tag = tag.expect("a tag");
            assert!(seen.insert(tag), "tag {tag} is reused");
        }
        for float in [0.0, -0.0, 1.5, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            assert!(!Value::float(float).is_tagged());
        }
    }

    #[test]
    fn every_heap_value_has_a_header_of_the_right_kind() {
        let mut heap = Heap::new();
        let _int = heap.int(1);
        let _text = heap.string("x");
        let _list = heap.list(Vec::new());
        let _tuple = heap.tuple(Vec::new());
        let _set = heap.set(Vec::new());
        let _dict = heap.dict(Vec::new());
        let _object = heap.object("C", Vec::new());
        let _function = heap.function("f", 0);
        let kinds: Vec<HeapKind> = (0..heap.object_count())
            .map(|index| heap.object_kind(index).expect("a registered object"))
            .collect();
        assert_eq!(
            kinds,
            vec![
                HeapKind::Int,
                HeapKind::String,
                HeapKind::List,
                HeapKind::Tuple,
                HeapKind::Set,
                HeapKind::Dict,
                HeapKind::Object,
                HeapKind::Function,
            ]
        );
        assert_eq!(heap.marked_count(), 0);
    }

    #[test]
    fn display_and_repr_match_the_interpreter() {
        let mut heap = Heap::new();
        let int = heap.int(1);
        let float = Value::float(1.5);
        let whole = Value::float(1.0);
        let text = heap.string("hi");
        let list = heap.list(vec![int, text, Value::boolean(true)]);
        let tuple = heap.tuple(vec![Value::null()]);
        let two = heap.int(2);
        let three = heap.int(3);
        let set = heap.set(vec![two, three]);
        let key = heap.string("a");
        let dict = heap.dict(vec![(key, list)]);
        let age = heap.int(30);
        let object = heap.object("Person", vec![("age".to_string(), age)]);
        let function = heap.function("twice", 1);

        assert_eq!(Value::null().display(), "null");
        assert_eq!(Value::boolean(false).display(), "False");
        assert_eq!(int.display(), "1");
        assert_eq!(float.display(), "1.5");
        assert_eq!(whole.display(), "1.0");
        assert_eq!(text.display(), "hi");
        assert_eq!(list.display(), "[1, \"hi\", True]");
        assert_eq!(tuple.display(), "(null)");
        assert_eq!(set.display(), "{2, 3}");
        assert_eq!(dict.display(), "{\"a\": [1, \"hi\", True]}");
        assert_eq!(object.display(), "Person {age: 30}");
        assert_eq!(function.display(), "twice");
        assert_eq!(text.repr(), "\"hi\"");
    }

    #[test]
    fn equality_matches_the_interpreters_representation_rule() {
        let mut heap = Heap::new();
        assert!(value_eq(heap.int(1), heap.int(1)));
        assert!(!value_eq(heap.int(1), Value::float(1.0)));
        assert!(value_eq(Value::float(f64::NAN), Value::float(f64::NAN)));
        assert!(value_eq(Value::null(), Value::null()));
        assert!(!value_eq(Value::null(), Value::boolean(false)));
        assert!(value_eq(heap.string("a"), heap.string("a")));
        let one = heap.int(1);
        let x = heap.string("x");
        let left = heap.list(vec![one, x]);
        let right = heap.list(vec![one, x]);
        assert!(value_eq(left, right));
        let one = heap.int(1);
        let list = heap.list(vec![one]);
        let tuple = heap.tuple(vec![one]);
        assert!(!value_eq(list, tuple));
        let key = heap.string("a");
        let one = heap.int(1);
        let left = heap.dict(vec![(key, one)]);
        let key = heap.string("a");
        let one = heap.int(1);
        let right = heap.dict(vec![(key, one)]);
        assert!(value_eq(left, right));
    }

    #[test]
    fn only_a_bool_is_a_condition() {
        let mut heap = Heap::new();
        assert_eq!(Value::boolean(true).condition(), Some(true));
        assert_eq!(Value::boolean(false).condition(), Some(false));
        assert_eq!(heap.int(1).condition(), None);
        assert_eq!(Value::null().condition(), None);
        assert_eq!(heap.string("").condition(), None);
        assert_eq!(heap.list(Vec::new()).condition(), None);
    }

    #[test]
    fn string_helpers_match_the_interpreter_rules() {
        let mut heap = Heap::new();
        let left = heap.string("ab");
        let right = heap.string("cd");
        let joined = heap.concat_strings(left, right).expect("two strings");
        assert_eq!(joined.display(), "abcd");

        let sliced = heap.slice_string(joined, 1, 3).expect("a char range");
        assert_eq!(sliced.display(), "bc");
        assert!(heap.slice_string(joined, 0, 99).is_none());

        assert_eq!(
            Heap::compare_strings(left, right),
            Some(Ordering::Less),
            "lexicographic byte order"
        );
        assert_eq!(
            Heap::compare_strings(left, heap.string("ab")),
            Some(Ordering::Equal)
        );
    }
}
