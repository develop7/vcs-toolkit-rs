# vcs-git

Automate the **Git** CLI from Rust through process execution. Part of the
[vcs-toolkit-rs](https://github.com/ZelAnton/vcs-toolkit-rs) workspace.

Typed, repo-scoped commands over the `git` binary, behind a **mockable
interface**. Commands run inside an OS job (via `vcs-process`) so no `git`
subprocess is ever orphaned.

```rust
use vcs_git::{Git, GitApi};
use std::path::Path;

let git = Git::new();
let branch = git.current_branch(Path::new("."));   // io::Result<String>
let status = git.status(Path::new("."));           // io::Result<Vec<StatusEntry>>
```

Consumers depend on the `GitApi` trait and substitute a fake in tests — enable
the `mock` feature for a `mockall`-generated `MockGitApi`, or inject a fake
process runner with `Git::with_runner(vcs_process::ScriptedRunner::new()…)`.

Requires the `git` binary on `PATH`.

## License

MIT
