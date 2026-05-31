//! The execution boundary as an async trait, so consumers can inject a fake
//! process runner in tests instead of spawning real binaries.
//!
//! - [`JobRunner`] is the real, job-backed runner (the default).
//! - [`ScriptedRunner`] is a dependency-free test double: map a command to a
//!   canned [`Output`].
//! - With the `mock` feature, `mockall` also generates a `MockRunner`.

use std::ffi::{OsStr, OsString};
use std::io;

use crate::{Exec, Output};

/// Runs a prepared [`Exec`] and returns its captured [`Output`].
///
/// Wrapper crates execute every command through a `Runner`, so a test can pass a
/// [`ScriptedRunner`] (or a `mockall` `MockRunner`) and exercise the real
/// argument-building and parsing without touching git/jj/gh.
#[cfg_attr(feature = "mock", mockall::automock)]
#[async_trait::async_trait]
pub trait Runner: Send + Sync {
    /// Execute `exec` and capture its result.
    async fn run(&self, exec: &Exec) -> io::Result<Output>;
}

/// The real runner: spawns the process inside a job (kill-on-close). The default
/// everywhere a `Runner` isn't explicitly supplied.
#[derive(Debug, Default, Clone, Copy)]
pub struct JobRunner;

#[async_trait::async_trait]
impl Runner for JobRunner {
    async fn run(&self, exec: &Exec) -> io::Result<Output> {
        exec.execute().await
    }
}

/// A test double mapping a command — matched by a prefix of its argument list —
/// to a canned [`Output`]. Build canned outputs with [`Output::ok`] /
/// [`Output::fail`].
#[derive(Debug, Default, Clone)]
pub struct ScriptedRunner {
    rules: Vec<(Vec<OsString>, Output)>,
    fallback: Option<Output>,
}

impl ScriptedRunner {
    /// An empty runner that errors on any unmatched command.
    pub fn new() -> Self {
        Self::default()
    }

    /// Reply with `out` when a run's arguments start with `args`.
    pub fn on<I, S>(mut self, args: I, out: Output) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let prefix = args
            .into_iter()
            .map(|a| a.as_ref().to_os_string())
            .collect();
        self.rules.push((prefix, out));
        self
    }

    /// Reply with `out` for any command no `on` rule matched.
    pub fn fallback(mut self, out: Output) -> Self {
        self.fallback = Some(out);
        self
    }
}

#[async_trait::async_trait]
impl Runner for ScriptedRunner {
    async fn run(&self, exec: &Exec) -> io::Result<Output> {
        let actual = exec.arguments();
        for (prefix, out) in &self.rules {
            if actual.len() >= prefix.len() && actual[..prefix.len()] == prefix[..] {
                return Ok(out.clone());
            }
        }
        self.fallback.clone().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("ScriptedRunner: no rule for args {actual:?}"),
            )
        })
    }
}
