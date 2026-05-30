# vcs-jj

Automate the **Jujutsu** (`jj`) CLI from Rust through process execution. Part of
the [vcs-toolkit-rs](https://github.com/ZelAnton/vcs-toolkit-rs) workspace.

Typed, repo-scoped commands over the `jj` binary, behind a **mockable
interface**. Commands run inside an OS job (via `vcs-process`) so no `jj`
subprocess is ever orphaned.

```rust
use vcs_jj::{Jj, JjApi};
use std::path::Path;

let jj = Jj::new();
let head = jj.current_change(Path::new("."));     // io::Result<Change>
jj.describe(Path::new("."), "message");           // io::Result<()>
```

Consumers depend on the `JjApi` trait and substitute a fake in tests — enable
the `mock` feature for a `mockall`-generated `MockJjApi`, or inject a fake
process runner with `Jj::with_runner(vcs_process::ScriptedRunner::new()…)`.

Requires the `jj` binary on `PATH`.

## License

MIT
