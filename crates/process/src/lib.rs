//! `vcs-process` — launch child processes inside an OS *job* so the whole
//! process tree dies with the parent. No orphaned `git`/`jj`/`gh` descendants
//! survive a crashing or exiting parent.
//!
//! The containment mechanism is platform-specific (see [`Mechanism`]):
//!
//! - **Windows**: a [Job Object] with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
//! - **Linux**: a [cgroup v2] killed via `cgroup.kill`, falling back to a POSIX
//!   process group when no writable cgroup is available.
//! - **other**: a plain spawn with no containment.
//!
//! v1 guarantees **kill-on-close**: terminating or dropping a [`Job`] kills
//! every process still inside it. Resource limits are out of scope for now, but
//! the type is structured to grow them later.
//!
//! [Job Object]: https://learn.microsoft.com/windows/win32/procthread/job-objects
//! [cgroup v2]: https://docs.kernel.org/admin-guide/cgroup-v2.html

use std::ffi::OsStr;
use std::io;
use std::process::Command;

// One platform module is compiled per target; each exposes the same `Job` shape
// (`new`/`spawn`/`kill_all`/`mechanism` + a kill-on-close `Drop`).
#[cfg_attr(windows, path = "windows.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
#[cfg_attr(not(any(windows, target_os = "linux")), path = "other.rs")]
mod imp;

mod exec;
pub use exec::{Exec, Output};

mod runner;
#[cfg(feature = "mock")]
pub use runner::MockRunner;
pub use runner::{JobRunner, Runner, ScriptedRunner};

/// Which OS mechanism a [`Job`] is actually using to contain its processes.
///
/// Surfaced so callers can tell when Linux silently fell back from a cgroup to a
/// process group (e.g. on a CI runner without cgroup delegation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Mechanism {
    /// Windows Job Object with kill-on-close.
    JobObject,
    /// Linux cgroup v2 torn down via `cgroup.kill`.
    CgroupV2,
    /// POSIX process group — the Linux fallback when no cgroup is writable.
    ProcessGroup,
    /// No containment: the child is spawned directly (non-Windows/Linux).
    None,
}

/// A handle to an OS job that owns a tree of child processes.
///
/// Dropping the `Job` kills every process still inside it (kill-on-close), so an
/// exiting or panicking parent never leaks subprocesses.
pub struct Job(imp::Job);

impl Job {
    /// Create a fresh, empty job.
    pub fn new() -> io::Result<Self> {
        imp::Job::new().map(Job)
    }

    /// Spawn `cmd` as a member of this job and return its handle.
    ///
    /// The child — and any process it later spawns — belongs to the job and is
    /// reaped when the job is killed or dropped.
    pub fn spawn(&self, cmd: &mut Command) -> io::Result<Child> {
        self.0.spawn(cmd).map(Child)
    }

    /// Kill every process currently in the job. Idempotent; the job stays usable
    /// only as a handle to close (further spawns are not expected after this).
    pub fn kill_all(&self) -> io::Result<()> {
        self.0.kill_all()
    }

    /// The containment mechanism actually in effect (see [`Mechanism`]).
    pub fn mechanism(&self) -> Mechanism {
        self.0.mechanism()
    }
}

/// A child process spawned into a [`Job`]. Thin wrapper over
/// [`std::process::Child`].
pub struct Child(std::process::Child);

impl Child {
    /// Wait for the process to exit.
    pub fn wait(&mut self) -> io::Result<std::process::ExitStatus> {
        self.0.wait()
    }

    /// Wait for exit and collect captured stdout/stderr (consumes the handle).
    pub fn wait_with_output(self) -> io::Result<std::process::Output> {
        self.0.wait_with_output()
    }

    /// Check whether the process has exited yet, without blocking.
    pub fn try_wait(&mut self) -> io::Result<Option<std::process::ExitStatus>> {
        self.0.try_wait()
    }

    /// Kill just this process. The job still governs the rest of the tree.
    pub fn kill(&mut self) -> io::Result<()> {
        self.0.kill()
    }

    /// Borrow the underlying [`std::process::Child`] (e.g. for its stdio pipes).
    pub fn inner_mut(&mut self) -> &mut std::process::Child {
        &mut self.0
    }
}

/// Run `binary <args>` inside a one-shot job and return trimmed stdout on
/// success.
///
/// Fails if the process can't be spawned (e.g. `binary` not on `PATH`) or exits
/// with a non-zero status — stderr is surfaced in the error message. The job is
/// dropped before returning, so any descendant that outlived `binary` is killed.
///
/// A thin shim over [`Exec`]; use the builder directly for a working directory,
/// env vars, or stdin input.
pub fn run<I, S>(binary: &str, args: I) -> io::Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Exec::new(binary).args(args).run()
}

/// Run `binary <args>` inside a one-shot job and capture its [`Output`] without
/// erroring on a non-zero exit — for commands whose exit code is meaningful.
pub fn output<I, S>(binary: &str, args: I) -> io::Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Exec::new(binary).args(args).output()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    // --- Hermetic unit tests (no subprocess) -------------------------------

    // Construct an `ExitStatus` for the given exit code without spawning
    // anything — keeps these tests hermetic. Windows takes the code directly;
    // Unix wants the raw wait status, where the exit code lives in bits 8–15.
    fn fake_status(code: i32) -> std::process::ExitStatus {
        #[cfg(windows)]
        {
            use std::os::windows::process::ExitStatusExt;
            std::process::ExitStatus::from_raw(code as u32)
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            std::process::ExitStatus::from_raw(code << 8)
        }
    }

    #[test]
    fn output_combined_concatenates_streams() {
        let out = Output {
            status: fake_status(0),
            stdout: "out".to_string(),
            stderr: "err".to_string(),
        };
        assert!(out.success());
        assert_eq!(out.combined(), "outerr");
    }

    #[test]
    fn output_into_result_trims_on_success() {
        let out = Output {
            status: fake_status(0),
            stdout: "  value\n".to_string(),
            stderr: String::new(),
        };
        assert_eq!(out.into_result().unwrap(), "value");
    }

    #[test]
    fn output_into_result_errors_with_stderr_on_failure() {
        let out = Output {
            status: fake_status(1),
            stdout: String::new(),
            stderr: "  boom\n".to_string(),
        };
        let err = out.into_result().unwrap_err();
        assert!(err.to_string().contains("boom"), "got: {err}");
    }

    // --- Subprocess tests (ignored on CI) ----------------------------------

    // Spawns a real subprocess, so it's ignored on CI. `cargo` is on PATH
    // wherever these tests run. Run locally with `cargo test -- --ignored`.
    #[test]
    #[ignore = "spawns a real subprocess"]
    fn run_captures_stdout() {
        let out = run("cargo", ["--version"]).expect("cargo should be installed");
        assert!(out.to_lowercase().contains("cargo"), "unexpected: {out}");
    }

    // Creates a real OS job object / cgroup, so it's ignored on CI.
    #[test]
    #[ignore = "creates an OS job object / cgroup"]
    fn job_reports_a_known_mechanism() {
        let job = Job::new().expect("job creation should succeed");
        assert!(
            matches!(
                job.mechanism(),
                Mechanism::JobObject
                    | Mechanism::CgroupV2
                    | Mechanism::ProcessGroup
                    | Mechanism::None
            ),
            "got {:?}",
            job.mechanism()
        );
    }

    // The core guarantee: dropping the job kills a process still inside it.
    // Exercises every backend (Job Object / cgroup / process group). Spawns a
    // real ~30s sleeper, so it's ignored on CI; run with `--ignored`.
    #[test]
    #[ignore = "spawns a long-lived subprocess and asserts kill-on-close"]
    fn dropping_job_kills_children() {
        use std::time::{Duration, Instant};

        let mut cmd = if cfg!(windows) {
            let mut c = Command::new("cmd");
            // `ping -n 30` blocks ~30s without needing a console (unlike timeout).
            c.args(["/c", "ping", "-n", "30", "127.0.0.1"]);
            c
        } else {
            let mut c = Command::new("sleep");
            c.arg("30");
            c
        };
        cmd.stdout(Stdio::null()).stderr(Stdio::null());

        let job = Job::new().expect("job creation");
        let mut child = job.spawn(&mut cmd).expect("spawn sleeper");
        assert!(
            child.try_wait().expect("try_wait").is_none(),
            "sleeper should still be running right after spawn"
        );

        drop(job); // kill-on-close should reap the child promptly

        let start = Instant::now();
        while child.try_wait().expect("try_wait").is_none() {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "child outlived its job — kill-on-close did not fire"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
