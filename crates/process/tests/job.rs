//! Integration tests for `vcs-process`: real subprocesses exercise `Exec`
//! options and the job's kill-on-close guarantee across backends.
//!
//! All ignored by default (they spawn real processes); run with
//! `cargo test -p vcs-process -- --ignored`.

mod common;

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::TempDir;
use vcs_process::{Exec, Job};

/// A ~30s sleeper command with output suppressed, per platform.
fn sleeper() -> Command {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/c", "ping", "-n", "30", "127.0.0.1"]);
        c
    } else {
        let mut c = Command::new("sleep");
        c.arg("30");
        c
    };
    cmd.stdout(Stdio::null()).stderr(Stdio::null());
    cmd
}

#[test]
#[ignore = "spawns a real subprocess"]
fn exec_runs_in_current_dir() {
    let tmp = TempDir::new("cwd");
    let segment = tmp
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();

    let out = if cfg!(windows) {
        Exec::new("cmd").args(["/c", "cd"])
    } else {
        // `-P` forces the physical path: `current_dir` chdir()s but doesn't update
        // the inherited `PWD`, so plain `pwd` could print the parent's directory.
        Exec::new("pwd").arg("-P")
    }
    .current_dir(tmp.path())
    .run()
    .expect("run in cwd");

    assert!(
        out.to_lowercase().contains(&segment.to_lowercase()),
        "cwd output {out:?} should mention {segment:?}"
    );
}

#[test]
#[ignore = "spawns a real subprocess"]
fn exec_passes_env() {
    let exec = if cfg!(windows) {
        Exec::new("cmd").args(["/c", "echo %VCS_TEST_ENV%"])
    } else {
        Exec::new("sh").args(["-c", "printf %s \"$VCS_TEST_ENV\""])
    };
    let out = exec
        .env("VCS_TEST_ENV", "hello-env")
        .run()
        .expect("run with env");
    assert!(out.contains("hello-env"), "got {out:?}");
}

#[test]
#[ignore = "spawns a real subprocess"]
fn exec_feeds_stdin() {
    // `sort` exists on both Windows and Unix and echoes a single line unchanged.
    let out = Exec::new("sort")
        .stdin("ping-pong")
        .run()
        .expect("feed stdin to sort");
    assert_eq!(out, "ping-pong");
}

#[test]
#[ignore = "spawns a real subprocess"]
fn output_captures_nonzero_exit() {
    let out = if cfg!(windows) {
        Exec::new("cmd").args(["/c", "exit 3"])
    } else {
        Exec::new("sh").args(["-c", "exit 3"])
    }
    .output()
    .expect("output() should not error on non-zero exit");

    assert!(!out.success());
    assert_eq!(out.status.code(), Some(3));
}

#[test]
#[ignore = "spawns a real subprocess"]
fn exec_spawn_feeds_and_closes_stdin() {
    // Regression guard: spawn() must write the buffered stdin and close the pipe.
    // Without the close, `sort` would block on stdin forever and this would hang.
    let (_job, child) = Exec::new("sort")
        .stdin("ping-pong")
        .spawn()
        .expect("spawn sort");
    let out = child.wait_with_output().expect("wait");
    assert!(out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("ping-pong"),
        "stdin was not fed through to stdout"
    );
}

#[test]
#[ignore = "spawns long-lived subprocesses and asserts kill-on-close"]
fn job_kills_multiple_children() {
    let job = Job::new().expect("job");
    let mut a = job.spawn(&mut sleeper()).expect("spawn a");
    let mut b = job.spawn(&mut sleeper()).expect("spawn b");
    assert!(a.try_wait().unwrap().is_none() && b.try_wait().unwrap().is_none());

    drop(job); // kill-on-close should reap both

    let start = Instant::now();
    loop {
        let a_done = a.try_wait().unwrap().is_some();
        let b_done = b.try_wait().unwrap().is_some();
        if a_done && b_done {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "children outlived the job"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

// The defining stray case: a grandchild that outlives its parent must still die
// with the job. Unix-only — `sh ... &` cleanly backgrounds a sleeper and exposes
// its pid, and /proc gives a dependency-free liveness check. On Windows the Job
// Object kills the whole tree by construction (covered by the multi-child test).
#[cfg(unix)]
#[test]
#[ignore = "spawns a detached grandchild and asserts it is killed"]
fn stray_grandchild_killed_on_drop() {
    let tmp = TempDir::new("grandchild");
    let pidfile = tmp.path().join("pid");

    // Background a sleeper, record its pid, then let the parent shell exit.
    let script = format!("sleep 30 & echo $! > {}", pidfile.display());
    let (job, mut child) = Exec::new("sh")
        .args(["-c", &script])
        .spawn()
        .expect("spawn parent shell");
    child.wait().expect("parent shell exits");

    let pid: u32 = std::fs::read_to_string(&pidfile)
        .expect("pidfile")
        .trim()
        .parse()
        .expect("pid");
    assert!(is_alive(pid), "grandchild should be running before drop");

    drop(job); // kill-on-close should reach the reparented grandchild

    let start = Instant::now();
    while is_alive(pid) {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "grandchild {pid} outlived the job"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(unix)]
fn is_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}
