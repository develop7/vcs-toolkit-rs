# vcs-mcp

A [Model Context Protocol](https://modelcontextprotocol.io) **server** exposing
**git/jj** repository operations — and their **GitHub/GitLab/Gitea** forge — as
agent-callable **tools**. Part of the
[vcs-toolkit-rs](https://github.com/ZelAnton/vcs-toolkit-rs) workspace.

Built on the [`vcs-core`](https://crates.io/crates/vcs-core) (`Repo`) and
[`vcs-forge`](https://crates.io/crates/vcs-forge) (`Forge`) facades: each tool
wraps a typed operation and returns its DTO as JSON, so an agent harness drives a
repository through **structured, validated calls** instead of raw shell — with the
wrappers' argv injection guards still underneath. **Read tools are always
available; mutating tools are gated behind `--allow-write`** and annotated
`destructiveHint`.

> 📖 **Full guide:** [docs/mcp.md](https://github.com/ZelAnton/vcs-toolkit-rs/blob/main/docs/mcp.md)

## The binary

```text
vcs-mcp [--repo <path>] [--forge github|gitlab|gitea] [--allow-write] [--timeout <seconds>]
```

The server drives git through a **hardened** client (`Git::hardened()` — repo
hooks and config disabled, so serving a repository you didn't create can't run its
hooks) and bounds every command with `--timeout` (default 120s; `0` disables) so a
stalled fetch/forge call can't hang a request.

The server speaks MCP over **stdio**; point a client at it via an `mcpServers`
config entry:

```json
{
  "mcpServers": {
    "vcs": {
      "command": "vcs-mcp",
      "args": ["--repo", "/path/to/repo", "--allow-write"]
    }
  }
}
```

The forge is auto-detected from the repo's `origin` remote (works on a colocated
jj repo too); pass `--forge` to override. Without `--allow-write`, only the read
tools are callable.

## The library

`VcsMcpServer` is independently embeddable over any `rmcp` transport:

```rust
use vcs_core::Repo;
use vcs_mcp::VcsMcpServer;
use rmcp::{ServiceExt, transport::stdio};

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let repo = Repo::open(".")?;
let server = VcsMcpServer::new(repo, /* forge */ None, /* allow_write */ false);
server.serve(stdio()).await?.waiting().await?;
# Ok(()) }
```

**Runtime:** like [`vcs-watch`](https://crates.io/crates/vcs-watch), `vcs-mcp` uses
**tokio at runtime** (the rmcp server loop) — run it inside a tokio runtime.

## License

MIT
