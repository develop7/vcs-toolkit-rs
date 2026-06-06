# Fuzz targets

[`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz) (libFuzzer) targets for
the richest public parsers — the conflict-marker models in `vcs-git` and
`vcs-jj`. They complement the in-tree `proptest` property tests (which run in
the normal `cargo test` CI gate) with continuous coverage-guided fuzzing.

This crate is **excluded from the workspace** (`exclude = ["fuzz"]` in the root
`Cargo.toml`) because cargo-fuzz needs **nightly Rust + libFuzzer**, so it never
touches the stable build, the MSRV, or CI. Run it manually:

```bash
cargo install cargo-fuzz
cargo +nightly fuzz run git_conflict     # parse_conflicts panic-freedom + render roundtrip
cargo +nightly fuzz run jj_conflict      # the jj diff/snapshot grammar + side/base materializers
```

Each target asserts the same invariants the proptests check: never panic on
arbitrary bytes, and `render(parse(x)?) == x` byte-for-byte. A crash reproducer
lands in `fuzz/artifacts/`; minimise and add it as a regression unit test in the
relevant `conflict.rs`.

Artifacts, corpora, and the build dir are git-ignored.
