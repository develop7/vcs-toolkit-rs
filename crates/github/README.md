# vcs-github

Automate **GitHub** from Rust through the `gh` CLI and process execution. Part of
the [vcs-toolkit-rs](https://github.com/ZelAnton/vcs-toolkit-rs) workspace.

Typed, **async** commands over the GitHub CLI (`gh`) that deserialize
`gh … --json` output into structs, behind a **mockable interface**. Commands run
inside an OS job (via `vcs-process`) so no `gh` subprocess is ever orphaned,
return the structured `CommandError`, and honour an optional timeout.

Inside an async context (every method is `async`):

```rust
use vcs_github::{GitHub, GitHubApi};
use std::path::Path;

let gh = GitHub::new();
let prs = gh.pr_list(Path::new(".")).await?;   // Vec<PullRequest>
let authed = gh.auth_status().await?;          // bool
```

Consumers depend on the `GitHubApi` trait and substitute a fake in tests — enable
the `mock` feature for a `mockall`-generated `MockGitHubApi`, or inject a fake
process runner with `GitHub::with_runner(vcs_process::ScriptedRunner::new()…)`.

Requires the `gh` binary on `PATH` (authenticated via `gh auth login`).

## License

MIT
