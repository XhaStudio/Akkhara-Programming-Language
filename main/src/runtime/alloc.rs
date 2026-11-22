//! Bump allocation and object headers for the runtime value word.
//!
//! This is Step 2 scaffolding: no engine allocates here yet, so program
//! behavior is unchanged. It gives the NaN-boxed payloads a real home:
//!
//! * every heap object starts with a [`Header`] carrying its kind and GC mark
//!   bit, so a value word only needs the pointer;
//! * [`Heap`] bump-allocates payloads from 64 KiB chunks, 8-byte aligned;
//! * every allocated object is registered so a future mark-sweep collector can
//!   walk them (and so tests can assert the headers).
//!
//! The plan's Step 2 allows a bump allocator without collection: a million
//! iterations may grow memory. Mark-sweep is deferred to the step that starts
//! running compiled programs against this runtime (Step 3), so allocation
//! never frees an object individually today. Chunk memory and the payloads it
//! holds are released when the `Heap` is dropped.

#![allow(dead_code)] // scaffolding: the engines do not use it yet

use std::cell::Cell;

use crate::runtime::value::{
    HeapDict, HeapFunction, HeapInt, HeapList, HeapObject, HeapString,
};

/// The kind stored in an object header. The value word's tag says the same
/// thing; the header is what a GC needs when it only has a pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum HeapKind {
    Int,
    String,
    List,
    Tuple,
    Set,
    Dict,
    Object,
    Function,
}

/// The first field of every heap payload. `#[repr(C)]` on the payloads makes
/// its address equal to the allocation's address, which is what lets the GC
/// recover a payload's type from the value word alone.
#[repr(C)]
#[derive(Debug)]
pub(crate) struct Header {
    pub(crate) kind: HeapKind,
    pub(crate) marked: Cell<bool>,
}

impl Header {
    pub(crate) fn new(kind: HeapKind) -> Header {
        Header {
            kind,
            marked: Cell::new(false),
        }
    }
}

/// A heap payload: a `#[repr(C)]` struct whose first field is a [`Header`].
pub(crate) trait HeapPayload {
    const KIND: HeapKind;
    fn header(&self) -> &Header;
}

struct Chunk {
    bytes: Vec<u8>,
    used: usize,
}

/// The runtime heap: bump allocation in fixed-size chunks, with a registry of
/// every object for the future collector.
pub(crate) struct Heap {
    chunks: Vec<Chunk>,
    objects: Vec<*mut Header>,
    chunk_size: usize,
}

impl Heap {
    pub(crate) fn new() -> Heap {
        Heap::with_chunk_size(64 * 1024)
    }

    pub(crate) fn with_chunk_size(chunk_size: usize) -> Heap {
        assert!(chunk_size >= 64, "a chunk must fit at least one payload");
        Heap {
            chunks: Vec::new(),
            objects: Vec::new(),
            chunk_size,
        }
    }

    fn alloc_bytes(&mut self, size: usize, align: usize) -> *mut u8 {
        let align = align.max(8);
        let start = match self.chunks.last() {
            Some(chunk) => align_up(chunk.used, align),
            None => 0,
        };
        let fits = self
            .chunks
            .last()
            .is_some_and(|chunk| start + size <= chunk.bytes.len());
        if !fits {
            let size = self.chunk_size.max(size + align);
            self.chunks.push(Chunk {
                bytes: vec![0; size],
                used: 0,
            });
        }
        let chunk = self.chunks.last_mut().expect("a chunk was just created");
        let start = align_up(chunk.used, align);
        chunk.used = start + size;
        // `chunk.bytes` is never resized after creation, so the pointer stays
        // valid for as long as the Heap lives.
        unsafe { chunk.bytes.as_mut_ptr().add(start) }
    }

    fn alloc<T>(&mut self, value: T) -> *mut T {
        let pointer = self.alloc_bytes(std::mem::size_of::<T>(), std::mem::align_of::<T>())
            as *mut T;
        unsafe { pointer.write(value) };
        pointer
    }

    /// Allocate a payload and register its header. The caller builds the
    /// payload with `Header::new(kind)` already in its first field.
    pub(crate) fn alloc_payload<T: HeapPayload>(&mut self, value: T) -> *mut T {
        let pointer = self.alloc(value);
        let header = pointer as *mut Header;
        debug_assert_eq!(
            unsafe { (*header).kind },
            T::KIND,
            "a payload's header kind must match its type"
        );
        self.objects.push(header);
        pointer
    }

    /// How many heap objects have been allocated.
    pub(crate) fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// The kind of the `index`th allocation, in allocation order.
    pub(crate) fn object_kind(&self, index: usize) -> Option<HeapKind> {
        self.objects.get(index).map(|&header| unsafe { (*header).kind })
    }

    /// Mark one object, as a collector would when it reaches it.
    pub(crate) fn mark_object(&mut self, index: usize) {
        if let Some(&header) = self.objects.get(index) {
            unsafe { (*header).marked.set(true) };
        }
    }

    pub(crate) fn is_marked(&self, index: usize) -> bool {
        self.objects
            .get(index)
            .is_some_and(|&header| unsafe { (*header).marked.get() })
    }

    pub(crate) fn marked_count(&self) -> usize {
        self.objects
            .iter()
            .filter(|&&header| unsafe { (*header).marked.get() })
            .count()
    }

    pub(crate) fn clear_marks(&mut self) {
        for &header in &self.objects {
            unsafe { (*header).marked.set(false) };
        }
    }

    /// Bytes reserved by chunk allocations, for tests and future GC heuristics.
    pub(crate) fn reserved_bytes(&self) -> usize {
        self.chunks.iter().map(|chunk| chunk.bytes.len()).sum()
    }

    pub(crate) fn chunk_count(&self) -> usize {
        self.chunks.len()
    }
}

impl Default for Heap {
    fn default() -> Heap {
        Heap::new()
    }
}

impl Drop for Heap {
    fn drop(&mut self) {
        // Bump allocation does not run destructors, but the payloads own
        // Rust-side buffers (`Vec<u8>`, `Vec<Value>`), so drop them by their
        // header kind before the chunk memory goes away.
        for &header in &self.objects {
            unsafe {
                match (*header).kind {
                    HeapKind::Int => std::ptr::drop_in_place(header as *mut HeapInt),
                    HeapKind::String => std::ptr::drop_in_place(header as *mut HeapString),
                    HeapKind::List => std::ptr::drop_in_place(header as *mut HeapList),
                    HeapKind::Tuple => std::ptr::drop_in_place(header as *mut HeapList),
                    HeapKind::Set => std::ptr::drop_in_place(header as *mut HeapList),
                    HeapKind::Dict => std::ptr::drop_in_place(header as *mut HeapDict),
                    HeapKind::Object => std::ptr::drop_in_place(header as *mut HeapObject),
                    HeapKind::Function => std::ptr::drop_in_place(header as *mut HeapFunction),
                }
            }
        }
    }
}

fn align_up(value: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    (value + align - 1) & !(align - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[repr(C)]
    struct TestPayload {
        header: Header,
        value: u64,
    }

    impl HeapPayload for TestPayload {
        const KIND: HeapKind = HeapKind::Int;

        fn header(&self) -> &Header {
            &self.header
        }
    }

    fn payload(value: u64) -> TestPayload {
        TestPayload {
            header: Header::new(HeapKind::Int),
            value,
        }
    }

    #[test]
    fn bump_allocation_is_aligned_stable_and_chunked() {
        let mut heap = Heap::with_chunk_size(64);
        let mut pointers = Vec::new();
        for value in 0..200u64 {
            let pointer = heap.alloc_payload(payload(value));
            assert_eq!(pointer as usize % 8, 0, "8-byte alignment");
            pointers.push((pointer, value));
        }
        assert!(heap.chunk_count() > 1, "small chunks must grow");
        assert!(heap.reserved_bytes() >= heap.chunk_count() * 64);
        for (pointer, value) in pointers {
            assert_eq!(unsafe { (*pointer).value }, value, "payload moved");
        }
        assert_eq!(heap.object_count(), 200);
        assert_eq!(heap.object_kind(0), Some(HeapKind::Int));
    }

    #[test]
    fn every_allocation_is_registered_and_markable() {
        let mut heap = Heap::new();
        for _ in 0..3 {
            heap.alloc_payload(payload(7));
        }
        assert_eq!(heap.marked_count(), 0);
        heap.mark_object(1);
        assert_eq!(heap.marked_count(), 1);
        assert!(heap.is_marked(1));
        assert!(!heap.is_marked(0));
        heap.clear_marks();
        assert_eq!(heap.marked_count(), 0);
    }
}
