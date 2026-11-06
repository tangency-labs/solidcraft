//! `solidcraft-cli scx-worker` (hidden): runs one script of the MCP server's script mode
//! (`solidcraft-cli mcp --scx`, docs/mcp.md, "Script mode") in the sandboxed interpreter, talking to the server
//! over stdin and stdout. The server starts one per script. Before reading the script the worker
//! caps its own memory with the operating system, so a script that allocates without end ends
//! only this process, never the server: a Job Object on Windows, RLIMIT_AS on Linux and the BSDs.
//! On macOS, which does not enforce RLIMIT_AS, the time limit is the backstop.
//!
//! The Windows cap needs the Job Object API, which has no safe binding, hence the `unsafe_code`
//! allowance on this module (the CLI crate denies it; see its Cargo.toml).
#![allow(unsafe_code)]

use solidcraft_mcp::scx::sandbox;

pub fn run() -> Result<(), String> {
    cap_memory(sandbox::WORKER_MEMORY).map_err(|e| format!("scx-worker: could not cap memory: {e}"))?;
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    sandbox::worker(stdin.lock(), stdout.lock()).map_err(|e| format!("scx-worker: {e}"))
}

/// Puts this process in a Job Object that fails allocations past `bytes` and lets it start no
/// other process.
#[cfg(windows)]
fn cap_memory(bytes: usize) -> Result<(), String> {
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: CreateJobObjectW takes no pointers we own (null attributes, no name) and returns a
    // handle or null. The handle is left open on purpose: the job must outlive this function, and
    // the process holds it until it exits.
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        return Err(std::io::Error::last_os_error().to_string());
    }
    // SAFETY: a zeroed JOBOBJECT_EXTENDED_LIMIT_INFORMATION is a valid value (plain integers).
    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_PROCESS_MEMORY | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
    info.BasicLimitInformation.ActiveProcessLimit = 1;
    info.ProcessMemoryLimit = bytes;
    let size = u32::try_from(std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).map_err(|e| e.to_string())?;
    // SAFETY: `info` is a live JOBOBJECT_EXTENDED_LIMIT_INFORMATION of `size` bytes for the call's
    // duration, and `job` is the handle just created.
    if unsafe { SetInformationJobObject(job, JobObjectExtendedLimitInformation, std::ptr::from_ref(&info).cast(), size) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    // SAFETY: both handles are valid: `job` above, and the current-process pseudo handle.
    if unsafe { AssignProcessToJobObject(job, GetCurrentProcess()) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}

/// Caps this process's address space at `bytes` (allocations past it fail).
#[cfg(all(unix, not(target_os = "macos")))]
fn cap_memory(bytes: usize) -> Result<(), String> {
    use nix::sys::resource::{Resource, setrlimit};
    let bytes = u64::try_from(bytes).map_err(|e| e.to_string())?;
    setrlimit(Resource::RLIMIT_AS, bytes, bytes).map_err(|e| e.to_string())
}

/// macOS does not enforce RLIMIT_AS: the server's time limit is the backstop.
#[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
fn cap_memory(_bytes: usize) -> Result<(), String> {
    Ok(())
}
