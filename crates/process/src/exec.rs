//! Ergonomic async command builder on top of [`Job`]. Set a working directory,
//! env vars, stdin input, or a timeout, then choose between erroring on failure
//! ([`Exec::run`]) or capturing the status yourself ([`Exec::output`]). Every
//! process still runs inside a job, so kill-on-close holds.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use tokio::process::Command;

use crate::{Child, CommandError, Job, JobRunner, Runner};

/// Captured result of a finished process.
///
/// Unlike [`Exec::run`], producing an `Output` does **not** treat a non-zero
/// exit as an error — inspect [`Output::status`] / [`Output::success`] /
/// [`Output::timed_out`] yourself.
#[derive(Debug, Clone)]
pub struct Output {
    /// Exit status of the process. On a timeout this is a synthetic non-zero
    /// status (the process was killed); check [`Output::timed_out`] to tell.
    pub status: ExitStatus,
    /// Captured standard output, lossily decoded as UTF-8.
    pub stdout: String,
    /// Captured standard error, lossily decoded as UTF-8.
    pub stderr: String,
    /// `true` when the process was killed because its timeout elapsed.
    pub timed_out: bool,
}

impl Output {
    /// Whether the process exited successfully (zero status, not timed out).
    pub fn success(&self) -> bool {
        !self.timed_out && self.status.success()
    }

    /// `stdout` followed by `stderr`. Concatenation, not real-time interleaving.
    pub fn combined(&self) -> String {
        let mut s = String::with_capacity(self.stdout.len() + self.stderr.len());
        s.push_str(&self.stdout);
        s.push_str(&self.stderr);
        s
    }

    /// Build a successful `Output` carrying `stdout` — for scripting a
    /// [`ScriptedRunner`](crate::ScriptedRunner) or a mock in tests.
    #[cfg(any(unix, windows))]
    pub fn ok(stdout: impl Into<String>) -> Self {
        Output {
            status: synthetic_status(0),
            stdout: stdout.into(),
            stderr: String::new(),
            timed_out: false,
        }
    }

    /// Build a failed `Output` with exit `code` and `stderr` — test/mock helper.
    #[cfg(any(unix, windows))]
    pub fn fail(code: i32, stderr: impl Into<String>) -> Self {
        Output {
            status: synthetic_status(code),
            stdout: String::new(),
            stderr: stderr.into(),
            timed_out: false,
        }
    }

    fn from_std(out: std::process::Output) -> Self {
        Output {
            status: out.status,
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            timed_out: false,
        }
    }

    #[cfg(any(unix, windows))]
    fn timed_out() -> Self {
        Output {
            // 124 is the conventional "timed out" exit code (cf. GNU `timeout`).
            status: synthetic_status(124),
            stdout: String::new(),
            stderr: String::new(),
            timed_out: true,
        }
    }
}

/// Construct an `ExitStatus` for `code` without spawning a process. Windows
/// takes the code directly; Unix wants the raw wait status (exit code in bits
/// 8–15). Available on every process-capable target (Unix incl. macOS/BSD, Windows).
#[cfg(any(unix, windows))]
fn synthetic_status(code: i32) -> ExitStatus {
    #[cfg(windows)]
    {
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(code as u32)
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(code << 8)
    }
}

/// Builder for a single job-backed command run.
///
/// ```no_run
/// # async fn example() -> Result<(), vcs_process::CommandError> {
/// let out = vcs_process::Exec::new("git")
///     .args(["status", "--porcelain"])
///     .current_dir("/path/to/repo")
///     .run()
///     .await?;
/// # Ok(()) }
/// ```
pub struct Exec {
    program: OsString,
    args: Vec<OsString>,
    cwd: Option<PathBuf>,
    envs: Vec<(OsString, OsString)>,
    stdin: Option<Vec<u8>>,
    timeout: Option<Duration>,
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
            timeout: None,
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
    /// Intended for modest inputs (a commit message, a small patch).
    pub fn stdin(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.stdin = Some(bytes.into());
        self
    }

    /// Kill the process (and its job) if it runs longer than `timeout`.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Apply an optional timeout (e.g. a client default). `None` leaves it unset.
    pub fn maybe_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }

    /// The program this run will execute.
    pub fn program(&self) -> &OsStr {
        &self.program
    }

    /// The arguments, in order — e.g. for a [`Runner`](crate::Runner) to match on.
    pub fn arguments(&self) -> &[OsString] {
        &self.args
    }

    /// The working-directory override, if one was set.
    pub fn working_dir(&self) -> Option<&Path> {
        self.cwd.as_deref()
    }

    fn joined_args(&self) -> String {
        self.args
            .iter()
            .map(|a| a.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Configure a [`tokio::process::Command`] from this builder. stdin is piped
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
        // Kill the direct child if its `Child` is dropped without being awaited —
        // e.g. when a timeout cancels the wait future. The job kills the whole
        // tree on Windows/Linux; this guarantees at least the child dies on every
        // platform (incl. the no-containment `other` path) and that `Exec::spawn`
        // never leaks an abandoned process.
        cmd.kill_on_drop(true);
        cmd
    }

    /// Run to completion and capture output, **without** erroring on a non-zero
    /// exit. On timeout the job is killed and `Output::timed_out` is set. The job
    /// is dropped before returning (kill-on-close).
    pub async fn output(self) -> io::Result<Output> {
        self.execute().await
    }

    /// The actual job-backed execution. Borrows `self` so [`JobRunner`] can run a
    /// borrowed `Exec` (the [`Runner`](crate::Runner) seam) without consuming it.
    pub(crate) async fn execute(&self) -> io::Result<Output> {
        let job = Job::new()?;
        let mut cmd = self.build();
        let mut child = job.spawn(&mut cmd)?;

        // Feed and close any buffered stdin (so the child sees EOF) and then
        // drain its output. The timeout, when set, covers this whole interaction
        // — including a stdin write that could otherwise block on a child that
        // never reads it.
        let drive = async {
            if let Some(input) = &self.stdin
                && let Some(mut sink) = child.inner_mut().stdin.take()
            {
                use tokio::io::AsyncWriteExt;
                sink.write_all(input).await?;
                sink.shutdown().await?;
            }
            child.wait_with_output().await
        };

        match self.timeout {
            Some(timeout) => match tokio::time::timeout(timeout, drive).await {
                Ok(result) => Ok(Output::from_std(result?)),
                Err(_elapsed) => {
                    // Deadline hit: kill the whole job, report a timed-out result.
                    let _ = job.kill_all();
                    Ok(Output::timed_out())
                }
            },
            None => Ok(Output::from_std(drive.await?)),
        }
    }

    /// Capture [`Output`] using an injected `runner` (the [`Runner`] seam),
    /// **without** erroring on a non-zero exit.
    pub async fn output_with<R: Runner + ?Sized>(self, runner: &R) -> io::Result<Output> {
        runner.run(&self).await
    }

    /// Run via an injected `runner`, returning the successful [`Output`] or a
    /// [`CommandError`] on a non-zero exit, timeout, or spawn failure. This is the
    /// mapping the wrapper crates build their typed commands on.
    pub async fn checked_with<R: Runner + ?Sized>(self, runner: &R) -> crate::Result<Output> {
        let program = self.program.to_string_lossy().into_owned();
        match runner.run(&self).await {
            Err(source) => Err(CommandError::Spawn { program, source }),
            Ok(out) if out.timed_out => Err(CommandError::Timeout {
                program,
                args: self.joined_args(),
                timeout: self.timeout.unwrap_or_default(),
            }),
            Ok(out) if out.success() => Ok(out),
            Ok(out) => Err(CommandError::Exit {
                program,
                args: self.joined_args(),
                code: out.status.code().unwrap_or(-1),
                stderr: out.stderr.trim().to_string(),
            }),
        }
    }

    /// Run to completion, returning trimmed stdout on success or a
    /// [`CommandError`] on a non-zero exit, timeout, or spawn failure.
    pub async fn run(self) -> crate::Result<String> {
        Ok(self
            .checked_with(&JobRunner)
            .await?
            .stdout
            .trim()
            .to_string())
    }

    /// Spawn into a fresh job and hand back both, for streaming or long-running
    /// processes. Any buffered [`stdin`](Exec::stdin) input is written and closed
    /// before returning; the timeout does **not** apply here (you drive the wait).
    pub async fn spawn(self) -> io::Result<(Job, Child)> {
        let job = Job::new()?;
        let mut cmd = self.build();
        let mut child = job.spawn(&mut cmd)?;
        if let Some(input) = &self.stdin
            && let Some(mut sink) = child.inner_mut().stdin.take()
        {
            use tokio::io::AsyncWriteExt;
            sink.write_all(input).await?;
            sink.shutdown().await?;
        }
        Ok((job, child))
    }
}
