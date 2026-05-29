# vcs-git

Automate the **Git** CLI from Rust through process execution. Part of the
[vcs-toolkit-rs](https://github.com/ZelAnton/vcs-toolkit-rs) workspace.

Thin, dependency-free wrappers that shell out to the `git` binary and capture
its output.

```rust
let version = vcs_git::version()?;        // `git --version`
let status = vcs_git::run(["status", "--short"])?;
```

Requires the `git` binary on `PATH`.

## License

MIT
