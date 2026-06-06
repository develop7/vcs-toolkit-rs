#![no_main]
//! Fuzz `vcs_git::conflict::parse_conflicts`: it must never panic on arbitrary
//! bytes (a real conflicted file from a git we don't control), and whatever
//! parses must re-render byte-for-byte.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let _ = vcs_git::conflict::has_conflict_markers(text);
    if let Ok(segments) = vcs_git::conflict::parse_conflicts(text) {
        assert_eq!(
            vcs_git::conflict::render(&segments),
            text,
            "render is not the inverse of parse"
        );
    }
});
