//! Ergonomic command builder on top of [`Job`]. Set a working directory, env
//! vars, or stdin input, then choose between erroring on a non-zero exit
//! ([`Exec::run`]) or capturing the status yourself ([`Exec::output`]). Every
//! process still runs inside a job, so kill-on-close holds.

use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use crate::{Child, Job};

/// Captured result of a finished process.
///
/// Unlike [`Exec::run`], producing an `Output` does **not** treat a non-zero
/// exit as an error — inspect [`Output::status`] (or [`Output::success`])
/// yourself. This is the right type for commands whose exit code is meaningful
/// (e.g. `git diff --quiet`).
#[derive(Debug, Clone)]
pub struct Output {
    /// Exit status of the process.
    pub status: ExitStatus,
    /// Captured standard output, lossily decoded as UTF-8.
    pub stdout: String,
    /// Captured standard error, lossily decoded as UTF-8.
    pub stderr: String,
}

impl Output {
    /// Whether the process exited successfully (zero status).
    pub fn success(&self) -> bool {
        self.status.success()
    }

    /// `stdout` followed by `stderr`. The streams are captured separately, so
    /// this is concatenation, not real-time interleaving.
    pub fn combined(&self) -> String {
        let mut s = String::with_capacity(self.stdout.len() + self.stderr.len());
        s.push_str(&self.stdout);
        s.push_str(&self.stderr);
        s
    }

    /// Apply the [`run`](crate::run) contract: trimmed stdout on success, or an
    /// `io::Error` carrying trimmed stderr on a non-zero exit.
    pub fn into_result(self) -> io::Result<String> {
        if self.status.success() {
            Ok(self.stdout.trim().to_string())
        } else {
            Err(io::Error::other(format!(
                "process exited with {}: {}",
                self.status,
                self.stderr.trim()
            )))
        }
    }
}

/// Builder for a single job-backed command run.
///
/// ```no_run
/// let out = vcs_process::Exec::new("git")
///     .args(["status", "--porcelain"])
///     .current_dir("/path/to/repo")
///     .run()?;
/// # Ok::<(), std::io::Error>(())
/// ```
pub struct Exec {
    program: OsString,
    args: Vec<OsString>,
    cwd: Option<PathBuf>,
    envs: Vec<(OsString, OsString)>,
    stdin: Option<Vec<u8>>,
}

impl Exec {
    /// Start building a run of `program`.
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        Exec {
            program: program.as_ref().to_os_string(),
            args: Vec::new(),
            cwd: None,
            envs: Vec::new(),
            stdin: None,
        }
    }

    /// Append one argument.
    pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.args.push(arg.as_ref().to_os_string());
        self
    }

    /// Append several arguments.
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.args
            .extend(args.into_iter().map(|a| a.as_ref().to_os_string()));
        self
    }

    /// Run the command in `dir` instead of the current working directory.
    pub fn current_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.cwd = Some(dir.as_ref().to_path_buf());
        self
    }

    /// Set one environment variable for the child.
    pub fn env(mut self, key: impl AsRef<OsStr>, val: impl AsRef<OsStr>) -> Self {
        self.envs
            .push((key.as_ref().to_os_string(), val.as_ref().to_os_string()));
        self
    }

    /// Set several environment variables for the child.
    pub fn envs<I, K, V>(mut self, vars: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.envs.extend(
            vars.into_iter()
                .map(|(k, v)| (k.as_ref().to_os_string(), v.as_ref().to_os_string())),
        );
        self
    }

    /// Feed `bytes` to the child's stdin (then close it, sending EOF).
    ///
    /// Intended for modest inputs (a commit message, a small patch): the bytes
    /// are written before the output is drained, so an input large enough to
    /// fill the OS pipe buffer could deadlock. Use [`Exec::spawn`] for streaming.
    pub fn stdin(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.stdin = Some(bytes.into());
        self
    }

    /// Configure a [`std::process::Command`] from this builder. stdin is piped
    /// when input was supplied, otherwise nulled (see [`run`](crate::run)).
    fn build(&self) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args);
        if let Some(dir) = &self.cwd {
            cmd.current_dir(dir);
        }
        for (k, v) in &self.envs {
            cmd.env(k, v);
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        cmd.stdin(if self.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        cmd
    }

    /// Run to completion and capture output, **without** erroring on a non-zero
    /// exit. The job is dropped before returning (kill-on-close).
    pub fn output(self) -> io::Result<Output> {
        let job = Job::new()?;
        let mut cmd = self.build();
        let mut child = job.spawn(&mut cmd)?;

        // Take and drop the handle so the child sees EOF before we drain it.
        if let Some(input) = &self.stdin
            && let Some(mut sink) = child.inner_mut().stdin.take()
        {
            sink.write_all(input)?;
        }

        let out = child.wait_with_output()?;
        Ok(Output {
            status: out.status,
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }

    /// Run to completion, returning trimmed stdout on success or an `io::Error`
    /// carrying stderr on a non-zero exit (the [`run`](crate::run) contract).
    pub fn run(self) -> io::Result<String> {
        let program = self.program.clone();
        let out = self.output()?;
        if out.status.success() {
            Ok(out.stdout.trim().to_string())
        } else {
            Err(io::Error::other(format!(
                "`{}` exited with {}: {}",
                program.to_string_lossy(),
                out.status,
                out.stderr.trim()
            )))
        }
    }

    /// Spawn into a fresh job and hand back both, for streaming or long-running
    /// processes. Any buffered [`stdin`](Exec::stdin) input is written and the
    /// pipe closed before returning (same modest-size caveat as `stdin`); if you
    /// gave none, the child's stdin is null.
    pub fn spawn(self) -> io::Result<(Job, Child)> {
        let job = Job::new()?;
        let mut cmd = self.build();
        let mut child = job.spawn(&mut cmd)?;
        // Feed and close any buffered stdin before handing the child back.
        if let Some(input) = &self.stdin
            && let Some(mut sink) = child.inner_mut().stdin.take()
        {
            sink.write_all(input)?;
        }
        Ok((job, child))
    }
}
