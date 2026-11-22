//! Grapheme-cluster helpers for text.
//!
//! Akkhara is a Myanmar-language language, and Myanmar script is written in
//! syllables: a consonant carries its medials, vowel signs, tone marks and
//! `်`/`္` marks, which Unicode encodes as separate scalar values. `မ` + `ာ`
//! renders as one visible character (`မာ`), so a program that counted scalar
//! values would report six characters for `မြန်မာ` and `slice` would cut a
//! syllable in half.
//!
//! Unicode's own extended grapheme clusters are close but not enough: they
//! break before several Myanmar vowel signs (U+102C among them), which splits
//! `မာ` into two clusters even though it is one character to a reader. So a
//! Myanmar cluster here is the script's syllable — a base character plus every
//! following mark that attaches to it, and, after the virama `္`, the stacked
//! consonant that follows.
//!
//! Everything a program can measure or cut -- `len`, `s[i]`, `slice`,
//! `split(s, "")`, `find`, `reverse`, `enumerate`, `for`, `map`/`filter` --
//! goes through this module, so they all agree on what "one character" means.
//! For Latin and emoji the unit is the usual grapheme cluster.
//!
//! One deliberate exception: `file.size` counts **bytes**, because that is
//! what a file's size is; `len` counts characters.
//!
//! Cluster boundaries depend only on the text, never on interpreter state, so
//! they are cached by content. A program that indexes or measures the same
//! string in a loop (`s[i]`, `len(s)`) pays for one pass over its graphemes,
//! not one per read: 5k index reads from a 3072-syllable string cost 1376 ms
//! before the cache and 4 ms after (benchmarks/BASELINE.md). The cache key is
//! the text itself, so a changed string is simply a different key -- there is
//! no stale entry to invalidate, and a mutation costs nothing to "clear". The
//! cache is thread-local, bounded in entries and bytes, and never changes the
//! result of any function.

use std::cell::RefCell;
use std::rc::Rc;
use unicode_segmentation::UnicodeSegmentation;

/// True for the Myanmar characters that attach to the character before them
/// (vowel signs, tone marks, medials, asat and virama) instead of starting a
/// new one. Ranges follow the Myanmar block's combining marks in Unicode.
fn attaches_to_previous(c: char) -> bool {
    matches!(
        c as u32,
        0x102B..=0x103E // vowel signs, tone marks, medials, asat, virama
        | 0x1056..=0x1059
        | 0x105E..=0x1060
        | 0x1062..=0x1064
        | 0x1067..=0x106D
        | 0x1071..=0x1074
        | 0x1082..=0x108D
        | 0x108F
        | 0x109A..=0x109D
        | 0xAA7B..=0xAA7D
        | 0xA9E5
    )
}

/// The virama (`္`), which stacks the next consonant onto this syllable.
fn is_virama(c: char) -> bool {
    c == '\u{1039}'
}

/// Whether `next` continues the syllable `current` rather than starting a new
/// one: a Myanmar mark always attaches, and after a virama the next consonant
/// (a stacked letter) belongs to the same syllable.
fn joins(current: &str, next: &str) -> bool {
    next.chars().next().is_some_and(attaches_to_previous)
        || current.chars().last().is_some_and(is_virama)
}

/// The byte offset each cluster starts at, in order. One pass over the
/// graphemes; everything else in this module is derived from this list.
fn build_starts(text: &str) -> Vec<usize> {
    let mut starts: Vec<usize> = Vec::new();
    let mut last_start = 0usize;
    let mut last_end = 0usize;
    let mut pos = 0usize;
    for grapheme in text.graphemes(true) {
        let start = pos;
        let end = start + grapheme.len();
        pos = end;
        if starts.is_empty() {
            starts.push(start);
            last_start = start;
            last_end = end;
        } else if joins(&text[last_start..last_end], grapheme) {
            last_end = end;
        } else {
            starts.push(start);
            last_start = start;
            last_end = end;
        }
    }
    starts
}

/// How many texts' cluster boundaries to keep, and how many bytes they may
/// hold. Both bounds are small: a program normally works on one or two long
/// strings at a time, and a shorter string's list should not be evicted by a
/// long one that is read once.
const MAX_CACHED_TEXTS: usize = 8;
const MAX_CACHED_BYTES: usize = 1 << 20;

struct StartsCache {
    /// Most recently used first.
    entries: Vec<(String, Rc<[usize]>)>,
    bytes: usize,
}

impl StartsCache {
    fn new() -> Self {
        StartsCache {
            entries: Vec::new(),
            bytes: 0,
        }
    }

    fn get(&mut self, text: &str) -> Option<Rc<[usize]>> {
        // Compare the length first: it is a couple of instructions, while the
        // content compare of a long string is real work even when it hits.
        let index = self
            .entries
            .iter()
            .position(|(key, _)| key.len() == text.len() && key == text)?;
        let entry = self.entries.remove(index);
        let starts = Rc::clone(&entry.1);
        self.entries.insert(0, entry);
        Some(starts)
    }

    fn insert(&mut self, text: &str, starts: Rc<[usize]>) {
        let size = entry_size(text, &starts);
        if size > MAX_CACHED_BYTES {
            // Too large to keep. Not caching is always correct; the caller
            // still gets the list it asked for.
            return;
        }
        self.bytes += size;
        self.entries.insert(0, (text.to_string(), starts));
        while self.entries.len() > MAX_CACHED_TEXTS || self.bytes > MAX_CACHED_BYTES {
            match self.entries.pop() {
                Some((key, starts)) => self.bytes -= entry_size(&key, &starts),
                None => break,
            }
        }
    }
}

fn entry_size(text: &str, starts: &[usize]) -> usize {
    text.len() + std::mem::size_of_val(starts)
}

thread_local! {
    /// The interpreter runs a program on one thread, and the cache is pure
    /// data derived from the text, so a thread-local is enough here -- no
    /// lock, and no sharing between program runs.
    static STARTS: RefCell<StartsCache> = RefCell::new(StartsCache::new());
}

/// Cluster boundaries for `text`, from the cache when this text was seen
/// before. Empty for empty text.
fn starts(text: &str) -> Rc<[usize]> {
    if let Some(hit) = STARTS.with(|cache| cache.borrow_mut().get(text)) {
        return hit;
    }
    let built: Rc<[usize]> = build_starts(text).into();
    STARTS.with(|cache| cache.borrow_mut().insert(text, Rc::clone(&built)));
    built
}

/// The cluster count of `text`, without touching the cache. `find` counts the
/// clusters of a prefix, and those prefixes would otherwise evict the string
/// the program is actually working on.
fn cluster_count(text: &str) -> usize {
    build_starts(text).len()
}

/// The grapheme clusters of `text`, in order. A Myanmar syllable is one
/// cluster. ASCII text has a fast path that allocates nothing beyond the
/// result.
pub fn clusters(text: &str) -> Vec<&str> {
    if text.is_ascii() {
        return text
            .as_bytes()
            .iter()
            .enumerate()
            .map(|(i, _)| &text[i..i + 1])
            .collect();
    }
    let starts = starts(text);
    let mut out = Vec::with_capacity(starts.len());
    for (i, &start) in starts.iter().enumerate() {
        let end = starts.get(i + 1).copied().unwrap_or(text.len());
        out.push(&text[start..end]);
    }
    out
}

/// How many characters (grapheme clusters) `text` has.
pub fn len(text: &str) -> i64 {
    if text.is_ascii() {
        return text.len() as i64;
    }
    starts(text).len() as i64
}

/// The cluster at `index`, if there is one. A negative index never names a
/// cluster; the interpreter reports that as out of range, like a list.
pub fn at(text: &str, index: usize) -> Option<&str> {
    if text.is_ascii() {
        return text.get(index..index + 1);
    }
    let starts = starts(text);
    let start = *starts.get(index)?;
    let end = starts.get(index + 1).copied().unwrap_or(text.len());
    Some(&text[start..end])
}

/// The clusters from `start` up to `end` (exclusive), joined back into text.
/// The caller clamps both ends to the length, exactly as it does for a list.
pub fn slice(text: &str, start: usize, end: usize) -> String {
    if text.is_ascii() {
        let end = end.min(text.len());
        let start = start.min(end);
        return text[start..end].to_string();
    }
    let starts = starts(text);
    let end = end.min(starts.len());
    let start = start.min(end);
    if start == end {
        return String::new();
    }
    let byte_start = starts[start];
    let byte_end = starts.get(end).copied().unwrap_or(text.len());
    text[byte_start..byte_end].to_string()
}

/// The cluster index of the first occurrence of `needle`, or `-1`. The index
/// is in clusters, so it lines up with `len` and `s[i]`; `str::find` answers
/// in bytes, which would be a different number for Myanmar text.
pub fn find(text: &str, needle: &str) -> i64 {
    match text.find(needle) {
        // `find` returns a byte offset on a character boundary, so the prefix
        // is valid text and counting its clusters is exact.
        Some(byte) if text.is_ascii() => byte as i64,
        Some(byte) => cluster_count(&text[..byte]) as i64,
        None => -1,
    }
}

/// The text with its clusters in reverse order. Reversing scalar values would
/// put a syllable's vowel sign before its consonant (`မာ` would become `ာမ`),
/// so cluster order is the only reversal that keeps the script readable.
pub fn reverse(text: &str) -> String {
    if text.is_ascii() {
        return text.chars().rev().collect();
    }
    clusters(text).into_iter().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `မြန်မာ` is three syllables: မြ, န်, မာ. Six scalar values, three
    /// characters -- which is the whole point of this module.
    #[test]
    fn a_myanmar_syllable_is_one_character() {
        let text = "မြန်မာ";
        assert_eq!(text.chars().count(), 6);
        assert_eq!(len(text), 3);
        assert_eq!(at(text, 0), Some("မြ"));
        assert_eq!(at(text, 2), Some("မာ"));
        assert_eq!(at(text, 3), None);
        assert_eq!(slice(text, 0, 2), "မြန်");
        assert_eq!(slice(text, 1, 3), "န်မာ");
        assert_eq!(reverse(text), "မာန်မြ");
        assert_eq!(find(text, "မာ"), 2);
        assert_eq!(find(text, "မြ"), 0);
        assert_eq!(find(text, "zz"), -1);
    }

    /// A vowel sign after a consonant is part of the same syllable, which
    /// Unicode's own cluster rules do not guarantee for Myanmar.
    #[test]
    fn vowel_signs_stay_with_their_consonant() {
        assert_eq!(clusters("မာ"), vec!["မာ"]);
        assert_eq!(clusters("ကို"), vec!["ကို"]);
        assert_eq!(clusters("မင်္ဂလာပါ"), vec!["မ", "င်္ဂ", "လာ", "ပါ"]);
    }

    #[test]
    fn ascii_and_combining_text_keep_their_usual_meaning() {
        assert_eq!(len("hello"), 5);
        assert_eq!(at("hello", 1), Some("e"));
        assert_eq!(slice("hello", 1, 3), "el");
        assert_eq!(reverse("abc"), "cba");
        assert_eq!(find("hello", "ll"), 2);

        // e + combining acute is one character, not two.
        let accented = "e\u{301}";
        assert_eq!(accented.chars().count(), 2);
        assert_eq!(len(accented), 1);
        assert_eq!(reverse(accented), accented);
    }

    #[test]
    fn splitting_into_clusters_matches_len() {
        let text = "မြန်မာ";
        assert_eq!(clusters(text).len() as i64, len(text));
        assert_eq!(clusters(text), vec!["မြ", "န်", "မာ"]);
        assert_eq!(clusters("abc"), vec!["a", "b", "c"]);
    }

    /// The cache is keyed by content, so growing a string must not reuse the
    /// boundaries of the text it used to be -- and the same bytes in another
    /// allocation must reuse them.
    #[test]
    fn cached_boundaries_follow_the_text() {
        let mut text = String::from("မြန်မာ");
        assert_eq!(len(&text), 3);
        assert_eq!(at(&text, 1), Some("န်"));
        text.push_str("ပါ");
        assert_eq!(len(&text), 4);
        assert_eq!(at(&text, 3), Some("ပါ"));
        assert_eq!(slice(&text, 2, 4), "မာပါ");

        // Equal content in a different allocation: same answer.
        let copy = "မြန်မာပါ".to_string();
        assert_eq!(len(&copy), 4);
        assert_eq!(at(&copy, 3), Some("ပါ"));

        // A longer text with the old text as its prefix is its own key.
        assert_eq!(len("မြန်မာပါပါ"), 5);
        assert_eq!(at("မြန်မာပါပါ", 4), Some("ပါ"));
    }

    #[test]
    fn emoji_and_combining_sequences_keep_their_units() {
        assert_eq!(len("👍🏽"), 1);
        assert_eq!(at("👍🏽", 0), Some("👍🏽"));
        assert_eq!(slice("👍🏽x", 1, 2), "x");
        assert_eq!(len("👨‍👩‍👧"), 1);
        assert_eq!(len("e\u{301}x"), 2);
        assert_eq!(reverse("e\u{301}x"), "xe\u{301}");
    }

    /// Entries are evicted past the bounds; answers must stay correct for
    /// every text, cached or not.
    #[test]
    fn evicted_entries_do_not_change_answers() {
        for n in 1..20usize {
            let text = "မာ".repeat(n);
            assert_eq!(len(&text), n as i64);
            assert_eq!(at(&text, n - 1), Some("မာ"));
            assert_eq!(slice(&text, 0, n), text);
        }
    }
}
