# vcs-github

Automate **GitHub** from Rust through the `gh` CLI and process execution. Part of
the [vcs-toolkit-rs](https://github.com/ZelAnton/vcs-toolkit-rs) workspace.

Thin, dependency-free wrappers that shell out to the GitHub CLI (`gh`) and
capture its output.

```rust
let version = vcs_github::version()?;     // `gh --version`
let prs = vcs_github::run(["pr", "list", "--json", "number,title"])?;
```

Requires the `gh` binary on `PATH` (authenticated via `gh auth login`).

## License

MIT
