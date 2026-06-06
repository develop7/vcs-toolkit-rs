#![no_main]
//! Fuzz `vcs_jj::conflict::parse_conflicts`: it must never panic on arbitrary
//! bytes, whatever parses must re-render byte-for-byte, and the side/base
//! materializers (which apply the recorded diff) must not panic either.

use libfuzzer_sys::fuzz_target;
use vcs_jj::conflict::{JjConflictSegment, has_conflict_markers, parse_conflicts, render};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let _ = has_conflict_markers(text);
    if let Ok(segments) = parse_conflicts(text) {
        assert_eq!(render(&segments), text, "render is not the inverse of parse");
        for segment in &segments {
            if let JjConflictSegment::Conflict(region) = segment {
                let _ = region.sides();
                let _ = region.base();
            }
        }
    }
});
