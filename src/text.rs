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

/// The grapheme clusters of `text`, in order. A Myanmar syllable is one
/// cluster. ASCII text has a fast path that allocates nothing.
pub fn clusters(text: &str) -> Vec<&str> {
    if text.is_ascii() {
        return text
            .as_bytes()
            .iter()
            .enumerate()
            .map(|(i, _)| &text[i..i + 1])
            .collect();
    }
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut pos = 0usize;
    for grapheme in text.graphemes(true) {
        let start = pos;
        let end = start + grapheme.len();
        pos = end;
        match spans.last_mut() {
            Some(last) if joins(&text[last.0..last.1], grapheme) => last.1 = end,
            _ => spans.push((start, end)),
        }
    }
    spans.into_iter().map(|(s, e)| &text[s..e]).collect()
}

/// Whether `next` continues the syllable `current` rather than starting a new
/// one: a Myanmar mark always attaches, and after a virama the next consonant
/// (a stacked letter) belongs to the same syllable.
fn joins(current: &str, next: &str) -> bool {
    next.chars().next().is_some_and(attaches_to_previous)
        || current.chars().last().is_some_and(is_virama)
}

/// How many characters (grapheme clusters) `text` has.
pub fn len(text: &str) -> i64 {
    if text.is_ascii() {
        return text.len() as i64;
    }
    clusters(text).len() as i64
}

/// The cluster at `index`, if there is one. A negative index never names a
/// cluster; the interpreter reports that as out of range, like a list.
pub fn at(text: &str, index: usize) -> Option<&str> {
    if text.is_ascii() {
        return text.get(index..index + 1);
    }
    clusters(text).into_iter().nth(index)
}

/// The clusters from `start` up to `end` (exclusive), joined back into text.
/// The caller clamps both ends to the length, exactly as it does for a list.
pub fn slice(text: &str, start: usize, end: usize) -> String {
    if text.is_ascii() {
        let end = end.min(text.len());
        let start = start.min(end);
        return text[start..end].to_string();
    }
    clusters(text)
        .into_iter()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

/// The cluster index of the first occurrence of `needle`, or `-1`. The index
/// is in clusters, so it lines up with `len` and `s[i]`; `str::find` answers
/// in bytes, which would be a different number for Myanmar text.
pub fn find(text: &str, needle: &str) -> i64 {
    match text.find(needle) {
        // `find` returns a byte offset on a character boundary, so the prefix
        // is valid text and counting its clusters is exact.
        Some(byte) if text.is_ascii() => byte as i64,
        Some(byte) => clusters(&text[..byte]).len() as i64,
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
}
