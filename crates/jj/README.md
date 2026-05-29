# vcs-jj

Automate the **Jujutsu** (`jj`) CLI from Rust through process execution. Part of
the [vcs-toolkit-rs](https://github.com/ZelAnton/vcs-toolkit-rs) workspace.

Thin, dependency-free wrappers that shell out to the `jj` binary and capture its
output.

```rust
let version = vcs_jj::version()?;         // `jj --version`
let status = vcs_jj::run(["status"])?;
```

Requires the `jj` binary on `PATH`.

## License

MIT
