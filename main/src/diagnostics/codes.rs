//! Every `E###` and `W###` the compiler can report, in one registry.
//!
//! A code means one thing and only one thing, and nothing is ever reused for
//! a second meaning. [`ALL`] is the complete set: a message may only carry a
//! code listed here, which the test at the bottom of this file enforces by
//! reading the source tree.
//!
//! The codes are grouped the way the pipeline reports them, not the way they
//! were added:
//!
//! | range | reported by |
//! |---|---|
//! | `E001`-`E013` | the lexer (`frontend::lexer`) |
//! | `E015`-`E096` | the parser (`frontend::parser`) |
//! | `E100`-`E158` | the interpreter (`interp`) |
//! | `E159`-`E172`, `E175` | the CLI, `akk build` and the bundle (`cli`, `project::bundle`, `vm`) |
//! | `E176` | the middle end: a construct `ir::mir` does not model yet |
//! | `E177` | the native backend (`backend`): a construct outside its slice |
//! | `E178` | a user-raised error (`ပစ်ပါ` / `throw`) |
//! | `E999` | the catch-all for a message that has no more specific code |
//! | `W001`-`W008` | the static warnings (`sema::warnings`) |

/// Every code in use, sorted so the list can be diffed.
#[allow(dead_code)] // data for the test below; nothing reads it at run time yet
pub(crate) const ALL: &[&str] = &[
    "E001", "E002", "E003", "E004", "E005", "E006", "E007", "E008", "E009", "E010", "E011", "E012",
    "E013", "E015", "E016", "E017", "E018", "E019", "E020", "E021", "E022", "E023", "E024", "E025",
    "E026", "E027", "E028", "E029", "E030", "E031", "E032", "E033", "E034", "E035", "E036", "E037",
    "E038", "E039", "E040", "E041", "E042", "E043", "E044", "E045", "E046", "E047", "E048", "E049",
    "E050", "E051", "E052", "E053", "E054", "E055", "E056", "E057", "E058", "E059", "E060", "E061",
    "E062", "E063", "E064", "E065", "E066", "E067", "E068", "E069", "E070", "E071", "E072", "E073",
    "E074", "E075", "E076", "E077", "E078", "E079", "E080", "E081", "E082", "E083", "E084", "E085",
    "E086", "E087", "E088", "E089", "E090", "E091", "E092", "E093", "E094", "E095", "E096", "E100",
    "E101", "E102", "E103", "E104", "E105", "E106", "E107", "E108", "E109", "E110", "E111", "E113",
    "E114", "E115", "E116", "E117", "E118", "E119", "E120", "E121", "E122", "E123", "E124", "E125",
    "E126", "E127", "E128", "E129", "E130", "E131", "E132", "E133", "E134", "E135", "E136", "E137",
    "E138", "E139", "E140", "E141", "E142", "E143", "E144", "E145", "E146", "E147", "E148", "E149",
    "E150", "E151", "E152", "E153", "E154", "E155", "E156", "E157", "E158", "E159", "E160", "E161",
    "E163", "E164", "E165", "E166", "E167", "E168", "E170", "E171", "E172", "E175", "E176", "E177",
    "E178", "E999", "W001", "W002", "W003", "W004", "W005", "W006", "W007", "W008",
];

/// Is `code` one of the registered codes?
#[allow(dead_code)] // the registry test is the only caller today
pub(crate) fn is_known(code: &str) -> bool {
    ALL.contains(&code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// Every `E###`/`W###` word in a `.rs` file under `src/`.
    fn codes_in(dir: &std::path::Path, found: &mut BTreeSet<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                codes_in(&path, found);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for word in text.split(|c: char| !c.is_ascii_alphanumeric()) {
                let bytes = word.as_bytes();
                if bytes.len() == 4
                    && (bytes[0] == b'E' || bytes[0] == b'W')
                    && bytes[1..].iter().all(u8::is_ascii_digit)
                {
                    found.insert(word.to_string());
                }
            }
        }
    }

    /// A code that appears in a message must be in the registry: one code,
    /// one meaning, and no message carrying a number nobody can look up.
    #[test]
    fn every_code_used_in_the_source_is_registered() {
        let mut found = BTreeSet::new();
        codes_in(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut found,
        );
        let missing: Vec<&String> = found.iter().filter(|code| !is_known(code)).collect();
        assert!(
            missing.is_empty(),
            "codes used in src but missing from diagnostics::codes::ALL: {:?}",
            missing
        );
    }

    /// The list is sorted and has no duplicates, so it can be diffed.
    #[test]
    fn the_registry_is_sorted_and_unique() {
        let mut sorted = ALL.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted, ALL.to_vec());
    }
}
