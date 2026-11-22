//! The runtime value word: 64-bit NaN-boxing (Step 2 spike).
//!
//! This module is scaffolding: no engine uses it yet, so a program's output is
//! unchanged. It fixes the representation the native runtime will use and
//! proves, by test, that every Akkhara value kind survives an encode/decode
//! round trip.
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
//! The spike boxes with `Box::into_raw` and intentionally leaks: the real
//! allocator/GC arrives with the production runtime. Nothing outside the
//! tests calls these constructors.

#![allow(dead_code)] // scaffolding: the engines do not use it yet

/// The one NaN bit pattern a float may keep. Every other NaN is folded here.
pub(crate) const CANONICAL_NAN: u64 = 0x7FF8_0000_0000_0000;

/// Bits 63..51: sign 1, exponent all ones, quiet bit 1. A tagged word always
/// has these bits set; a canonical NaN has the sign bit clear instead.
const TAG_MASK: u64 = 0xFFF8_0000_0000_0000;
/// Bits 46..0: an 8-byte-aligned pointer, or zero for the tag-only kinds.
const PAYLOAD_MASK: u64 = (1 << 47) - 1;
const TAG_SHIFT: u32 = 47;

const TAG_NULL: u64 = 0;
const TAG_FALSE: u64 = 1;
const TAG_TRUE: u64 = 2;
const TAG_INT: u64 = 3;
const TAG_STR: u64 = 4;
const TAG_LIST: u64 = 5;
const TAG_TUPLE: u64 = 6;
const TAG_SET: u64 = 7;
const TAG_DICT: u64 = 8;
const TAG_OBJECT: u64 = 9;
const TAG_FUNCTION: u64 = 10;

/// A class instance: its class name and its fields in declaration order.
#[derive(Debug)]
pub(crate) struct Object {
    pub(crate) class: String,
    pub(crate) fields: Vec<(String, Value)>,
}

/// A callable value. A closure's captured cells will extend this.
#[derive(Debug)]
pub(crate) struct Function {
    pub(crate) name: String,
    pub(crate) arity: usize,
}

/// One Akkhara value in one 64-bit word.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Value(u64);

impl Value {
    fn tagged(tag: u64, payload: u64) -> Value {
        debug_assert!(payload <= PAYLOAD_MASK, "payload exceeds 47 bits");
        debug_assert_eq!(payload & 0b111, 0, "heap pointers must be 8-byte aligned");
        Value(TAG_MASK | (tag << TAG_SHIFT) | payload)
    }

    fn boxed<T>(tag: u64, value: T) -> Value {
        let pointer = Box::into_raw(Box::new(value)) as u64;
        Value::tagged(tag, pointer)
    }

    pub(crate) fn null() -> Value {
        Value::tagged(TAG_NULL, 0)
    }

    pub(crate) fn boolean(value: bool) -> Value {
        Value::tagged(if value { TAG_TRUE } else { TAG_FALSE }, 0)
    }

    /// An `int` is a full `i64`, so it is boxed rather than squeezed into 47
    /// bits. The allocator may later add an inline small-int tag.
    pub(crate) fn int(value: i64) -> Value {
        Value::boxed(TAG_INT, value)
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

    pub(crate) fn string(value: impl Into<String>) -> Value {
        Value::boxed(TAG_STR, value.into())
    }

    pub(crate) fn list(values: Vec<Value>) -> Value {
        Value::boxed(TAG_LIST, values)
    }

    pub(crate) fn tuple(values: Vec<Value>) -> Value {
        Value::boxed(TAG_TUPLE, values)
    }

    pub(crate) fn set(values: Vec<Value>) -> Value {
        Value::boxed(TAG_SET, values)
    }

    pub(crate) fn dict(pairs: Vec<(Value, Value)>) -> Value {
        Value::boxed(TAG_DICT, pairs)
    }

    pub(crate) fn object(class: impl Into<String>, fields: Vec<(String, Value)>) -> Value {
        Value::boxed(
            TAG_OBJECT,
            Object {
                class: class.into(),
                fields,
            },
        )
    }

    pub(crate) fn function(name: impl Into<String>, arity: usize) -> Value {
        Value::boxed(
            TAG_FUNCTION,
            Function {
                name: name.into(),
                arity,
            },
        )
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
        (self.tag() == Some(TAG_INT)).then(|| unsafe { *(self.payload() as *const i64) })
    }

    pub(crate) fn as_float(self) -> Option<f64> {
        (!self.is_tagged()).then(|| f64::from_bits(self.0))
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        (self.tag() == Some(TAG_STR))
            .then(|| unsafe { &*(self.payload() as *const String) }.as_str())
    }

    fn sequence(self, wanted: u64) -> Option<&'static [Value]> {
        (self.tag() == Some(wanted)).then(|| unsafe {
            let values = &*(self.payload() as *const Vec<Value>);
            values.as_slice()
        })
    }

    pub(crate) fn as_list(&self) -> Option<&'static [Value]> {
        self.sequence(TAG_LIST)
    }

    pub(crate) fn as_tuple(&self) -> Option<&'static [Value]> {
        self.sequence(TAG_TUPLE)
    }

    pub(crate) fn as_set(&self) -> Option<&'static [Value]> {
        self.sequence(TAG_SET)
    }

    pub(crate) fn as_dict(&self) -> Option<&'static [(Value, Value)]> {
        (self.tag() == Some(TAG_DICT)).then(|| unsafe {
            let pairs = &*(self.payload() as *const Vec<(Value, Value)>);
            pairs.as_slice()
        })
    }

    pub(crate) fn as_object(&self) -> Option<&'static Object> {
        (self.tag() == Some(TAG_OBJECT)).then(|| unsafe { &*(self.payload() as *const Object) })
    }

    pub(crate) fn as_function(&self) -> Option<&'static Function> {
        (self.tag() == Some(TAG_FUNCTION)).then(|| unsafe { &*(self.payload() as *const Function) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_round_trips_through_one_word() {
        assert!(Value::null().is_null());
        assert_eq!(Value::boolean(true).as_bool(), Some(true));
        assert_eq!(Value::boolean(false).as_bool(), Some(false));

        for int in [0, 1, -1, 42, i64::MIN, i64::MAX, 1 << 46, -(1 << 46)] {
            assert_eq!(Value::int(int).as_int(), Some(int), "int {int}");
            assert!(Value::int(int).is_tagged());
        }

        for float in [0.0, -0.0, 1.5, -2.25, f64::INFINITY, f64::NEG_INFINITY] {
            let value = Value::float(float);
            assert!(!value.is_tagged(), "a float must stay inline");
            assert_eq!(value.as_float(), Some(float), "float {float}");
        }
        let nan = Value::float(f64::NAN);
        assert!(nan.as_float().expect("a float").is_nan());
        assert_eq!(nan.bits(), CANONICAL_NAN);

        let text = Value::string("မင်္ဂလာပါ {x}");
        assert_eq!(text.as_str(), Some("မင်္ဂလာပါ {x}"));

        let list = Value::list(vec![Value::int(1), Value::string("two")]);
        let items = list.as_list().expect("a list");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].as_int(), Some(1));
        assert_eq!(items[1].as_str(), Some("two"));

        let tuple = Value::tuple(vec![Value::boolean(true), Value::null()]);
        assert_eq!(tuple.as_tuple().expect("a tuple").len(), 2);

        let set = Value::set(vec![Value::int(7)]);
        assert_eq!(set.as_set().expect("a set").len(), 1);

        let dict = Value::dict(vec![(Value::string("a"), Value::int(1))]);
        let pairs = dict.as_dict().expect("a dict");
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0.as_str(), Some("a"));
        assert_eq!(pairs[0].1.as_int(), Some(1));

        let object = Value::object("Person", vec![("age".to_string(), Value::int(30))]);
        let object = object.as_object().expect("an object");
        assert_eq!(object.class, "Person");
        assert_eq!(object.fields[0].0, "age");
        assert_eq!(object.fields[0].1.as_int(), Some(30));

        let function = Value::function("twice", 1);
        let function = function.as_function().expect("a function");
        assert_eq!(function.name, "twice");
        assert_eq!(function.arity, 1);
    }

    #[test]
    fn the_word_is_eight_bytes_and_heap_payloads_are_aligned() {
        assert_eq!(std::mem::size_of::<Value>(), 8);
        assert_eq!(std::mem::align_of::<Value>(), 8);

        let heap_values = [
            Value::int(9),
            Value::string("x"),
            Value::list(Vec::new()),
            Value::tuple(Vec::new()),
            Value::set(Vec::new()),
            Value::dict(Vec::new()),
            Value::object("C", Vec::new()),
            Value::function("f", 0),
        ];
        for value in heap_values {
            assert!(value.is_tagged());
            assert_ne!(value.payload(), 0);
            assert_eq!(value.payload() & 0b111, 0, "8-byte alignment");
            assert!(value.payload() <= PAYLOAD_MASK);
        }
    }

    #[test]
    fn the_tags_are_distinct_and_outside_the_float_space() {
        let tags = [
            Value::null().tag(),
            Value::boolean(false).tag(),
            Value::boolean(true).tag(),
            Value::int(0).tag(),
            Value::string("").tag(),
            Value::list(Vec::new()).tag(),
            Value::tuple(Vec::new()).tag(),
            Value::set(Vec::new()).tag(),
            Value::dict(Vec::new()).tag(),
            Value::object("C", Vec::new()).tag(),
            Value::function("f", 0).tag(),
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
}
