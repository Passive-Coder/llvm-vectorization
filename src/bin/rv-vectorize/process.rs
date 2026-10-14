//! Bounded subprocess execution with captured diagnostics.
use super::Options;
use std::fs;
use std::io;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::NamedTempFile;

#[allow(clippy::too_many_lines)]
pub(super) fn run(
    command: &mut Command,
    label: &Path,
    options: &Options,
) -> Result<Output, String> {
    let stdout =
        NamedTempFile::new().map_err(|error| format!("cannot create stdout capture: {error}"))?;
    let stderr =
        NamedTempFile::new().map_err(|error| format!("cannot create stderr capture: {error}"))?;
    command.stdin(Stdio::null());
    command.stdout(Stdio::from(
        stdout
            .reopen()
            .map_err(|error| format!("cannot reopen stdout capture: {error}"))?,
    ));
    command.stderr(Stdio::from(
        stderr
            .reopen()
            .map_err(|error| format!("cannot reopen stderr capture: {error}"))?,
    ));

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        #[cfg(target_os = "linux")]
        let memory_bytes = options
            .max_memory_mib
            .map(|mib| mib.saturating_mul(1024 * 1024));
        // SAFETY: the closure calls only async-signal-safe libc functions before exec.
        unsafe {
            command.pre_exec(move || {
                if libc::setpgid(0, 0) != 0 {
                    return Err(io::Error::last_os_error());
                }
                #[cfg(target_os = "linux")]
                if let Some(bytes) = memory_bytes {
                    let limit = libc::rlimit {
                        rlim_cur: bytes,
                        rlim_max: bytes,
                    };
                    if libc::setrlimit(libc::RLIMIT_AS, &limit) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
    }
    #[cfg(not(unix))]
    if options.max_memory_mib.is_some() {
        return Err("--max-memory-mib is available only on Unix".to_owned());
    }

    let mut child = command.spawn().map_err(|error| {
        format!(
            "failed to execute {} for {}: {error}",
            command.get_program().to_string_lossy(),
            label.display()
        )
    })?;
    let started = Instant::now();
    let timeout = options.timeout_seconds.map(Duration::from_secs);
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("cannot wait for {}: {error}", label.display()))?
        {
            break status;
        }
        #[cfg(target_os = "macos")]
        if let Some(limit) = options.max_memory_mib {
            if resident_bytes(child.id())
                .is_some_and(|bytes| bytes > limit.saturating_mul(1024 * 1024))
            {
                // SAFETY: the child was started in a new process group with its PID as PGID.
                if let Ok(pid) = i32::try_from(child.id()) {
                    unsafe {
                        libc::kill(-pid, libc::SIGKILL);
                    }
                }
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{} exceeded --max-memory-mib ({limit} MiB)",
                    label.display()
                ));
            }
        }
        if timeout.is_some_and(|limit| started.elapsed() >= limit) {
            #[cfg(unix)]
            // SAFETY: the child was started in a new process group with its PID as PGID.
            if let Ok(pid) = i32::try_from(child.id()) {
                unsafe {
                    libc::kill(-pid, libc::SIGKILL);
                }
            }
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "{} timed out after {} seconds",
                label.display(),
                options.timeout_seconds.unwrap_or(0)
            ));
        }
        thread::sleep(Duration::from_millis(10));
    };
    let read_capture = |path: &Path| -> Result<Vec<u8>, String> {
        let metadata = fs::metadata(path)
            .map_err(|error| format!("cannot inspect process output: {error}"))?;
        if metadata.len() > 16 * 1024 * 1024 {
            return Err(format!(
                "process output for {} exceeds 16 MiB",
                label.display()
            ));
        }
        fs::read(path).map_err(|error| format!("cannot read process output: {error}"))
    };
    Ok(Output {
        status,
        stdout: read_capture(stdout.path())?,
        stderr: read_capture(stderr.path())?,
    })
}

#[cfg(target_os = "macos")]
fn resident_bytes(pid: u32) -> Option<u64> {
    let mut information = std::mem::MaybeUninit::<libc::proc_taskinfo>::uninit();
    let pid = i32::try_from(pid).ok()?;
    let size = i32::try_from(std::mem::size_of::<libc::proc_taskinfo>()).ok()?;
    // SAFETY: `information` points to writable memory of the exact requested size.
    let count = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTASKINFO,
            0,
            information.as_mut_ptr().cast(),
            size,
        )
    };
    if usize::try_from(count).ok()? != std::mem::size_of::<libc::proc_taskinfo>() {
        return None;
    }
    // SAFETY: proc_pidinfo initialized all bytes when it returned the full struct size.
    Some(unsafe { information.assume_init() }.pti_resident_size)
}
