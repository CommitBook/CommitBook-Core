//! Bounded subprocess execution for everything sync spawns while it holds
//! the repository lock: AI CLIs, `gh` probes, and commit signing.
//!
//! A child that never exits, or a grandchild that keeps its stdout open after
//! the child exits, must not hold the lock forever, so one deadline covers
//! the child's exit and draining its output. The child runs in its own
//! process group, and the whole group is killed when the deadline passes.

use anyhow::{Context, Result};
use std::io::{Read, Write};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

/// Run `command` to completion within `timeout`, writing `input` to its
/// stdin (or giving it no stdin) while stdout and stderr are drained
/// concurrently, so no pipe can fill up and deadlock. Blocks the calling
/// thread; async callers wrap it in `spawn_blocking`.
///
/// When the child has exited but stopped reading before all of `input` was
/// written, the broken pipe is ignored and the exit status decides success.
/// Callers that must know the child read everything use
/// `run_bounded_tracking_input`.
pub(crate) fn run_bounded(
    command: &mut Command,
    input: Option<&[u8]>,
    timeout: Duration,
) -> Result<Output> {
    run_bounded_tracking_input(command, input, timeout).map(|finished| finished.output)
}

/// Result of `run_bounded_tracking_input`.
pub(crate) struct Finished {
    pub output: Output,
    /// False when the child exited before reading all of the input.
    pub input_complete: bool,
}

/// `run_bounded`, also reporting whether the child read all of `input`.
pub(crate) fn run_bounded_tracking_input(
    command: &mut Command,
    input: Option<&[u8]>,
    timeout: Duration,
) -> Result<Finished> {
    let start = Instant::now();
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let program = command.get_program().to_string_lossy().into_owned();
    let mut child = command
        .spawn()
        .with_context(|| format!("Failed to start {program}"))?;
    let writer = input.map(|input| {
        let mut stdin = child.stdin.take().expect("piped stdin");
        let bytes = input.to_vec();
        std::thread::spawn(move || stdin.write_all(&bytes))
    });
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let out = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let err = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).map(|_| bytes)
    });

    let result = (|| -> Result<ExitStatus> {
        let mut status = None;
        loop {
            if status.is_none() {
                status = child.try_wait()?;
            }
            if let Some(status) = status {
                let written = writer.as_ref().is_none_or(|w| w.is_finished());
                if written && out.is_finished() && err.is_finished() {
                    return Ok(status);
                }
            }
            if start.elapsed() >= timeout {
                anyhow::bail!("{program} timed out after {timeout:?}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    let status = match result {
        Ok(status) => status,
        Err(error) => {
            // Also reaches grandchildren that inherited the output pipes, so
            // the reader threads see EOF and finish on their own.
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };

    let stdout = out
        .join()
        .map_err(|_| anyhow::anyhow!("stdout reader panicked"))??;
    let stderr = err
        .join()
        .map_err(|_| anyhow::anyhow!("stderr reader panicked"))??;
    let mut input_complete = true;
    if let Some(writer) = writer {
        match writer
            .join()
            .map_err(|_| anyhow::anyhow!("stdin writer panicked"))?
        {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => {
                input_complete = false;
            }
            Err(error) => {
                return Err(anyhow::Error::new(error)
                    .context(format!("Failed to write input to {program}")))
            }
        }
    }
    Ok(Finished {
        output: Output {
            status,
            stdout,
            stderr,
        },
        input_complete,
    })
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
