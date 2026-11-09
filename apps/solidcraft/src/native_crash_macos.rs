//! macOS: what a crash in native code leaves behind (docs/crash-reports.md).
//!
//! A crash inside a graphics driver or another library (a segfault, an `abort()`, a stack
//! overflow) is no Rust panic: the panic hook never runs and the next start only found "an
//! unclean exit". A signal handler for SIGSEGV, SIGBUS, SIGILL, SIGFPE, SIGTRAP (how a trap
//! instruction arrives on Apple silicon) and SIGABRT now writes, before the process goes, the
//! same small JSON file as the Windows filter (`native_crash.rs`): the signal, the faulting
//! module and the offset into it, and a short frame-pointer walk, next to the run's crash journal
//! (`solidcraft_engine::crash::native_paths`). The next start puts it into the unclean exit's
//! report. The signal then goes on to the handler that was there before (the standard library's
//! stack overflow message) or to the default action, so the process still dies of it and macOS
//! still writes its own crash report (`~/Library/Logs/DiagnosticReports`; the file names the
//! `pid` the JSON records).
//!
//! Installing a signal handler and asking dyld which image holds an address have no safe API,
//! hence the `unsafe_code` allowance on this module (the app crate denies it). The handler runs
//! on a thread that may have a broken heap, or on the small alternate stack after a stack
//! overflow: it doesn't allocate, formats into a buffer on the stack, and only reads memory on
//! the thread's own stack. It writes what it knows from the signal first and only then calls
//! `dladdr` (not on POSIX's async-signal-safe list; it takes dyld's recursive reader lock and
//! doesn't allocate) to name the modules, rewriting the file, so a hang or a fault in there
//! still leaves a record.
#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_int, c_void};
use std::fmt::Write as _;
use std::os::unix::ffi::OsStrExt;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};

use libc::{SIGABRT, SIGBUS, SIGFPE, SIGILL, SIGSEGV, SIGTRAP, siginfo_t};

/// The signals a crash arrives as, in the order of [`PREVIOUS`].
const SIGNALS: [c_int; 6] = [SIGSEGV, SIGBUS, SIGILL, SIGFPE, SIGTRAP, SIGABRT];
/// Frames the walk records (the faulting instruction first).
const MAX_FRAMES: usize = 24;
/// A fault this far below a thread's stack is taken for a stack overflow.
const GUARD: usize = 64 << 10;

/// Where the handler writes (NUL-terminated), set once by [`install`].
static PATH: OnceLock<CString> = OnceLock::new();
/// The handler that was installed before ours for each of [`SIGNALS`] (called after ours), as an
/// address (`SIG_DFL` is 0, `SIG_IGN` 1), and its `sa_flags`.
static PREVIOUS: [AtomicUsize; 6] = [const { AtomicUsize::new(libc::SIG_DFL) }; 6];
static PREVIOUS_FLAGS: [AtomicI32; 6] = [const { AtomicI32::new(0) }; 6];
/// The handler has started writing (a second signal while it does is only passed on).
static BUSY: AtomicBool = AtomicBool::new(false);

/// Catch native crashes from now on (after `solidcraft_engine::crash::install`; nothing without it).
pub fn install() {
    let Some((info, _)) = solidcraft_engine::crash::native_paths() else { return };
    let Ok(path) = CString::new(info.as_os_str().as_bytes()) else { return };
    if PATH.set(path).is_err() {
        return;
    }
    for (slot, &signal) in SIGNALS.iter().enumerate() {
        // SAFETY: `sigaction` is plain data; all zeros is a valid (empty) value.
        let mut ours: libc::sigaction = unsafe { std::mem::zeroed() };
        ours.sa_sigaction = handler as *const () as usize;
        // On the alternate stack the standard library gives each of its threads, so that a stack
        // overflow can still be recorded.
        ours.sa_flags = libc::SA_SIGINFO | libc::SA_ONSTACK;
        // SAFETY: as above.
        let mut old: libc::sigaction = unsafe { std::mem::zeroed() };
        // SAFETY: `handler` has the signature SA_SIGINFO calls it with and stays valid for the
        // process's life; both structs are valid for the call.
        if unsafe { libc::sigaction(signal, &ours, &mut old) } == 0
            && old.sa_sigaction != ours.sa_sigaction
            && let (Some(p), Some(f)) = (PREVIOUS.get(slot), PREVIOUS_FLAGS.get(slot))
        {
            f.store(old.sa_flags, Ordering::SeqCst);
            p.store(old.sa_sigaction, Ordering::SeqCst);
        }
    }
}

/// The signal handler: record, then pass the signal on.
unsafe extern "C" fn handler(signal: c_int, info: *mut siginfo_t, context: *mut c_void) {
    // SAFETY: `__error` is this thread's errno, restored for whatever runs after us.
    let errno = unsafe { *libc::__error() };
    let slot = SIGNALS.iter().position(|&s| s == signal).unwrap_or(0);
    let previous = PREVIOUS.get(slot).map_or(libc::SIG_DFL, |p| p.load(Ordering::SeqCst));
    // SAFETY: the system passes a valid siginfo and the interrupted thread's context for the
    // duration of an SA_SIGINFO handler.
    let f = unsafe { fault(signal, info, context) };
    // Someone sent a signal the app ignored before: still nothing to record.
    let ignored = previous == libc::SIG_IGN && !f.fault;
    if !ignored && !BUSY.swap(true, Ordering::SeqCst) {
        record(&f);
    }
    // SAFETY: as above.
    unsafe { pass_on(signal, slot, previous, f.fault, info, context) };
    // SAFETY: as above.
    unsafe { *libc::__error() = errno };
}

/// The signal comes from the faulting instruction itself (not `kill`, `raise` or `abort`), so
/// returning runs it again: on arm64, the exception the thread last took matches the signal.
/// macOS gives a SIGSEGV sent with `kill` the same `si_code` as a real one (SEGV_ACCERR), but
/// the exception is then the thread's last system call (or something long past, whose fault
/// address doesn't match).
#[cfg(any(target_arch = "aarch64", test))]
fn arm64_fault(signal: c_int, si_code: c_int, addr: usize, esr: u32, far: u64) -> bool {
    const ABORTS: [u32; 4] = [0x20, 0x21, 0x24, 0x25];
    const SVC: u32 = 0x15;
    let class = esr >> 26;
    signal != SIGABRT
        && (1..0x10000).contains(&si_code)
        && match signal {
            SIGSEGV | SIGBUS => ABORTS.contains(&class) && far as usize == addr,
            _ => class != SVC && !ABORTS.contains(&class),
        }
}

/// As [`arm64_fault`], from the x86 trap number and page-fault address.
#[cfg(any(target_arch = "x86_64", test))]
fn x86_fault(signal: c_int, si_code: c_int, addr: usize, trapno: u16, faultvaddr: u64) -> bool {
    signal != SIGABRT
        && (1..0x10000).contains(&si_code)
        && match signal {
            SIGSEGV | SIGBUS => (trapno == 14 && faultvaddr as usize == addr) || matches!(trapno, 12 | 13),
            SIGILL => trapno == 6,
            SIGFPE => matches!(trapno, 0 | 16 | 19),
            SIGTRAP => matches!(trapno, 1 | 3),
            _ => false,
        }
}

/// Hand the signal to the previous handler, or to the default action. A crash signal that was
/// only sent goes to the default action: the handler before ours (the standard library's, for
/// SIGSEGV and SIGBUS) expects a fault that happens again when it returns.
unsafe fn pass_on(signal: c_int, slot: usize, previous: usize, fault: bool, info: *mut siginfo_t, context: *mut c_void) {
    let ours = handler as *const () as usize;
    if previous != libc::SIG_DFL && previous != libc::SIG_IGN && previous != ours && (fault || signal == SIGABRT) {
        let flags = PREVIOUS_FLAGS.get(slot).map_or(0, |f| f.load(Ordering::SeqCst));
        if flags & libc::SA_SIGINFO != 0 {
            // SAFETY: the address came from `sigaction` as an SA_SIGINFO handler.
            let previous: unsafe extern "C" fn(c_int, *mut siginfo_t, *mut c_void) = unsafe { std::mem::transmute(previous) };
            // SAFETY: as for this handler.
            unsafe { previous(signal, info, context) };
        } else {
            // SAFETY: the address came from `sigaction` as a plain handler.
            let previous: unsafe extern "C" fn(c_int) = unsafe { std::mem::transmute(previous) };
            // SAFETY: as for this handler.
            unsafe { previous(signal) };
        }
        return;
    }
    if previous == libc::SIG_IGN && !fault {
        return;
    }
    // SAFETY: as in `install`.
    let mut default: libc::sigaction = unsafe { std::mem::zeroed() };
    default.sa_sigaction = libc::SIG_DFL;
    // SAFETY: a valid struct; restoring the default action is async-signal-safe.
    unsafe { libc::sigaction(signal, &default, std::ptr::null_mut()) };
    // A fault happens again when we return, now with the default action, so macOS reports it at
    // the instruction; anything else is raised again, and arrives once this handler returns.
    if !fault {
        // SAFETY: async-signal-safe.
        unsafe { libc::raise(signal) };
    }
}

/// The darwin `ucontext_t`, up to the pointer to the machine context (`<sys/_types/_ucontext.h>`).
#[repr(C)]
struct UContext {
    _onstack: c_int,
    _sigmask: u32,
    _stack: libc::stack_t,
    _link: *mut c_void,
    _mcsize: usize,
    mcontext: *const MContext,
}

/// The darwin arm64 machine context: exception state, then the general registers
/// (`<mach/arm/_structs.h>`; the floating-point state that follows isn't read).
#[cfg(target_arch = "aarch64")]
#[repr(C)]
struct MContext {
    far: u64,
    esr: u32,
    _exception: u32,
    _x: [u64; 29],
    fp: u64,
    _lr: u64,
    _sp: u64,
    pc: u64,
}

/// The darwin x86_64 machine context: exception state, then the general registers
/// (`<mach/i386/_structs.h>`; rax rbx rcx rdx rdi rsi rbp rsp r8–r15, then rip).
#[cfg(target_arch = "x86_64")]
#[repr(C)]
struct MContext {
    trapno: u16,
    _cpu: u16,
    err: u32,
    faultvaddr: u64,
    gpr: [u64; 16],
    rip: u64,
}

/// What the faulting instruction did to its address, when the processor says.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Access {
    Unknown,
    Read,
    Write,
    Execute,
}

/// What a crash signal says, read out of the signal before anything is looked up.
#[derive(Clone, Copy, Debug)]
struct Fault {
    signal: c_int,
    si_code: c_int,
    /// `si_addr`: the address that faulted (for SIGILL, SIGFPE and SIGTRAP, the instruction).
    addr: usize,
    pc: usize,
    fp: usize,
    access: Access,
    /// It comes from the faulting instruction (not `kill`, `raise` or `abort`).
    fault: bool,
    /// The process that sent the signal (0 or our own for a fault or an `abort()`).
    sender: i32,
    pid: i32,
    thread: u64,
    thread_name: [u8; 64],
    /// The thread's stack, `[low, high)` (0, 0 when unknown).
    stack: (usize, usize),
}

impl Fault {
    /// The fault hit just below the thread's stack: the stack ran out.
    fn overflow(&self) -> bool {
        let (low, high) = self.stack;
        matches!(self.signal, SIGSEGV | SIGBUS) && high > low && self.addr < low.saturating_add(4096) && self.addr >= low.saturating_sub(GUARD)
    }

    fn signal_name(&self) -> &'static str {
        match self.signal {
            SIGSEGV => "SIGSEGV",
            SIGBUS => "SIGBUS",
            SIGILL => "SIGILL",
            SIGFPE => "SIGFPE",
            SIGTRAP => "SIGTRAP",
            SIGABRT => "SIGABRT",
            _ => "signal",
        }
    }

    fn name(&self) -> &'static str {
        if self.overflow() {
            return "stack overflow";
        }
        match self.signal {
            SIGSEGV => "segmentation fault",
            SIGBUS => "bus error",
            SIGILL => "illegal instruction",
            SIGFPE => "arithmetic exception",
            SIGTRAP => "trap",
            SIGABRT => "abort",
            _ => "signal",
        }
    }

    /// The `si_code` names that say something about a crash.
    fn code_name(&self) -> Option<&'static str> {
        Some(match (self.signal, self.si_code) {
            (SIGSEGV, 1) => "SEGV_MAPERR",
            (SIGSEGV, 2) => "SEGV_ACCERR",
            (SIGBUS, 1) => "BUS_ADRALN",
            (SIGBUS, 2) => "BUS_ADRERR",
            (SIGBUS, 3) => "BUS_OBJERR",
            (SIGFPE, 7) => "FPE_INTDIV",
            (SIGFPE, 8) => "FPE_INTOVF",
            _ => return None,
        })
    }

    /// One line on what happened: the address read or written, a stack overflow, who sent it.
    fn detail(&self, b: &mut Buf) {
        if self.sender != 0 && self.sender != self.pid {
            let _ = write!(b, "sent by process {}", self.sender);
            return;
        }
        if !self.fault {
            if self.signal != SIGABRT {
                let _ = b.write_str("sent with kill or raise, not a fault");
            }
            return;
        }
        let what = match self.access {
            Access::Read => "read of",
            Access::Write => "write to",
            Access::Execute => "execution at",
            Access::Unknown => "access to",
        };
        if self.overflow() {
            let _ = write!(b, "{what} 0x{:016X}, below the thread's stack", self.addr);
        } else if matches!(self.signal, SIGSEGV | SIGBUS) {
            let _ = write!(b, "{what} 0x{:016X}", self.addr);
        } else {
            let _ = write!(b, "at 0x{:016X}", self.addr);
        }
        if let Some(c) = self.code_name() {
            let _ = write!(b, " ({c})");
        }
    }
}

/// An image that holds an address, as `dladdr` names it.
struct Module<'a> {
    path: &'a [u8],
    base: usize,
    symbol: Option<(&'a [u8], usize)>,
}

/// A fixed buffer to format into without allocating (what doesn't fit is cut off).
struct Buf {
    bytes: [u8; 8192],
    len: usize,
}

impl Buf {
    fn new() -> Buf {
        Buf { bytes: [0; 8192], len: 0 }
    }

    fn room(&self) -> usize {
        self.bytes.len().saturating_sub(self.len)
    }

    fn as_bytes(&self) -> &[u8] {
        self.bytes.get(..self.len).unwrap_or_default()
    }

    /// `s` as the inside of a JSON string, at most `max` bytes of it (bytes that aren't UTF-8
    /// and control characters become `?`).
    fn escaped(&mut self, s: &[u8], max: usize) {
        for chunk in s.get(..s.len().min(max)).unwrap_or_default().utf8_chunks() {
            for c in chunk.valid().chars() {
                let _ = match c {
                    '"' => self.write_str("\\\""),
                    '\\' => self.write_str("\\\\"),
                    c if (c as u32) < 0x20 => self.write_str("?"),
                    c => self.write_char(c),
                };
            }
            if !chunk.invalid().is_empty() {
                let _ = self.write_str("?");
            }
        }
    }
}

impl std::fmt::Write for Buf {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        let n = s.len().min(self.room());
        if let (Some(to), Some(from)) = (self.bytes.get_mut(self.len..self.len + n), s.as_bytes().get(..n)) {
            to.copy_from_slice(from);
            self.len += n;
        }
        Ok(())
    }
}

/// The last part of a path.
fn file_name(path: &[u8]) -> &[u8] {
    path.rsplit(|&c| c == b'/').next().unwrap_or(path)
}

/// The report: what the signal says, then (with `resolve`) the module of the faulting
/// instruction and the `frames`. Always valid JSON: frames stop while there is room to close it.
fn json(b: &mut Buf, f: &Fault, frames: &[usize], resolve: impl Fn(usize, bool) -> Option<Module<'static>>) {
    let signal = f.signal_name();
    let _ = write!(b, "{{\"signal\":\"{signal}\",\"number\":{},\"code\":\"{signal}\",\"name\":\"{}\"", f.signal, f.name());
    let _ = write!(b, ",\"si_code\":{},\"address\":\"0x{:016X}\",\"thread\":{},\"pid\":{}", f.si_code, f.pc, f.thread, f.pid);
    let name = f.thread_name.split(|&c| c == 0).next().unwrap_or_default();
    if !name.is_empty() {
        let _ = b.write_str(",\"thread_name\":\"");
        b.escaped(name, 64);
        let _ = b.write_str("\"");
    }
    let mark = b.len;
    let _ = b.write_str(",\"detail\":\"");
    let start = b.len;
    f.detail(b);
    if b.len == start {
        b.len = mark;
    } else {
        let _ = b.write_str("\"");
    }
    if let Some(m) = resolve(f.pc, false) {
        let _ = write!(b, ",\"offset\":\"0x{:X}\",\"module\":\"", f.pc.wrapping_sub(m.base));
        b.escaped(m.path, 300);
        let _ = b.write_str("\"");
    }
    if !frames.is_empty() {
        let _ = b.write_str(",\"frames\":[");
        for (i, &at) in frames.iter().enumerate() {
            // A frame is at most ~300 bytes; keep room for it and the closing `]}`.
            if b.room() < 400 {
                break;
            }
            if i > 0 {
                let _ = b.write_str(",");
            }
            let _ = b.write_str("\"");
            // A return address points after the call: look up the call itself.
            match resolve(at, i > 0) {
                Some(m) => {
                    b.escaped(file_name(m.path), 100);
                    let _ = write!(b, "+0x{:X}", at.wrapping_sub(m.base));
                    if let Some((symbol, start)) = m.symbol {
                        let _ = b.write_str(" ");
                        b.escaped(symbol, 160);
                        let _ = write!(b, "+0x{:X}", at.wrapping_sub(start));
                    }
                }
                None => {
                    let _ = write!(b, "0x{at:016X}");
                }
            }
            let _ = b.write_str("\"");
        }
        let _ = b.write_str("]");
    }
    let _ = b.write_str("}");
}

/// The return addresses up the frame-pointer chain from `fp`, after `pc`. `read` gives the word
/// at an address; frames outside `stack`, misaligned or not going up end the walk.
fn walk(pc: usize, fp: usize, stack: (usize, usize), read: impl Fn(usize) -> Option<usize>, out: &mut [usize; MAX_FRAMES]) -> usize {
    let (low, high) = stack;
    let mut n = 0;
    if let Some(slot) = out.get_mut(n) {
        *slot = pc;
        n += 1;
    }
    let mut fp = fp;
    while n < MAX_FRAMES {
        let in_stack = fp >= low && fp.checked_add(16).is_some_and(|end| end <= high);
        if !fp.is_multiple_of(8) || !in_stack {
            break;
        }
        let (Some(next), Some(ret)) = (read(fp), read(fp + 8)) else { break };
        let ret = strip_pointer_auth(ret);
        if ret == 0 {
            break;
        }
        if let Some(slot) = out.get_mut(n) {
            *slot = ret;
            n += 1;
        }
        if next <= fp {
            break;
        }
        fp = next;
    }
    n
}

/// Return addresses saved by arm64e code carry a signature in their top bits.
fn strip_pointer_auth(addr: usize) -> usize {
    if cfg!(target_arch = "aarch64") { addr & 0x0000_7FFF_FFFF_FFFF } else { addr }
}

/// Read what the signal and the interrupted thread's registers say.
unsafe fn fault(signal: c_int, info: *const siginfo_t, context: *const c_void) -> Fault {
    let mut f = Fault {
        signal,
        si_code: 0,
        addr: 0,
        pc: 0,
        fp: 0,
        access: Access::Unknown,
        fault: false,
        sender: 0,
        // SAFETY: async-signal-safe.
        pid: unsafe { libc::getpid() },
        thread: 0,
        thread_name: [0; 64],
        stack: (0, 0),
    };
    if !info.is_null() {
        // SAFETY: the system passes a valid siginfo for the duration of the handler.
        let info = unsafe { &*info };
        f.si_code = info.si_code;
        f.addr = info.si_addr as usize;
        f.sender = info.si_pid;
        // Without registers (never seen), the signal's own code has to do.
        f.fault = signal != SIGABRT && (1..0x10000).contains(&f.si_code);
    }
    let context = context as *const UContext;
    // SAFETY: an SA_SIGINFO handler's third argument is the interrupted thread's ucontext, whose
    // machine context pointer the kernel fills in.
    let mcontext = if context.is_null() { std::ptr::null() } else { unsafe { (*context).mcontext } };
    if !mcontext.is_null() {
        // SAFETY: as above.
        let m = unsafe { &*mcontext };
        #[cfg(target_arch = "aarch64")]
        {
            f.pc = strip_pointer_auth(m.pc as usize);
            f.fp = m.fp as usize;
            f.fault = arm64_fault(signal, f.si_code, f.addr, m.esr, m.far);
            // A data abort says whether it was a write (WnR); an instruction abort is execution.
            f.access = match m.esr >> 26 {
                0x24 | 0x25 if m.esr & (1 << 6) != 0 => Access::Write,
                0x24 | 0x25 => Access::Read,
                0x20 | 0x21 => Access::Execute,
                _ => Access::Unknown,
            };
        }
        #[cfg(target_arch = "x86_64")]
        {
            f.pc = m.rip as usize;
            f.fp = m.gpr.get(6).copied().unwrap_or(0) as usize;
            f.fault = x86_fault(signal, f.si_code, f.addr, m.trapno, m.faultvaddr);
            // A page fault's error code: bit 4 instruction fetch, bit 1 write.
            f.access = match (m.trapno, m.err) {
                (14, e) if e & 0x10 != 0 => Access::Execute,
                (14, e) if e & 0x2 != 0 => Access::Write,
                (14, _) => Access::Read,
                _ => Access::Unknown,
            };
        }
    }
    if f.pc == 0 && matches!(signal, SIGILL | SIGFPE | SIGTRAP) {
        f.pc = f.addr;
    }
    // SAFETY: these read the calling thread's own pthread structure; the buffers are as long as
    // we say.
    unsafe {
        let me = libc::pthread_self();
        libc::pthread_threadid_np(me, &mut f.thread);
        libc::pthread_getname_np(me, f.thread_name.as_mut_ptr().cast(), f.thread_name.len());
        let high = libc::pthread_get_stackaddr_np(me) as usize;
        let size = libc::pthread_get_stacksize_np(me);
        if let Some(low) = high.checked_sub(size) {
            f.stack = (low, high);
        }
    }
    f
}

/// The image that holds `addr` (the call before it, for a return address).
fn resolve(addr: usize, return_address: bool) -> Option<Module<'static>> {
    let at = if return_address { addr.saturating_sub(1) } else { addr };
    // SAFETY: `Dl_info` is plain data; all zeros is a valid value.
    let mut info: libc::Dl_info = unsafe { std::mem::zeroed() };
    // SAFETY: `dladdr` only reads dyld's image list; the strings it returns live as long as the
    // image, which a crashing process never unloads.
    unsafe {
        if libc::dladdr(at as *const c_void, &mut info) == 0 || info.dli_fname.is_null() {
            return None;
        }
        let symbol = (!info.dli_sname.is_null()).then(|| (CStr::from_ptr(info.dli_sname).to_bytes(), info.dli_saddr as usize));
        Some(Module { path: CStr::from_ptr(info.dli_fname).to_bytes(), base: info.dli_fbase as usize, symbol })
    }
}

/// Write what the signal says into the info file, then again with the modules and frames.
fn record(f: &Fault) {
    let Some(path) = PATH.get() else { return };
    // SAFETY: NUL-terminated path; the descriptor is closed below.
    let fd = unsafe { libc::open(path.as_ptr(), libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_CLOEXEC, 0o600) };
    if fd < 0 {
        return;
    }
    let mut b = Buf::new();
    json(&mut b, f, &[], |_, _| None);
    let bytes = b.as_bytes();
    // SAFETY: the buffer is as long as we say.
    unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
    let mut frames = [0usize; MAX_FRAMES];
    // SAFETY: `walk` only reads inside the thread's own stack, 8-byte aligned.
    let n = walk(f.pc, f.fp, f.stack, |at| Some(unsafe { (at as *const usize).read() }), &mut frames);
    b.len = 0;
    json(&mut b, f, frames.get(..n).unwrap_or_default(), resolve);
    let bytes = b.as_bytes();
    // SAFETY: as above.
    unsafe {
        if libc::pwrite(fd, bytes.as_ptr().cast(), bytes.len(), 0) == bytes.len() as isize {
            libc::ftruncate(fd, bytes.len() as libc::off_t);
        }
        libc::close(fd);
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;
    use std::path::{Path, PathBuf};

    use super::*;

    fn fault(signal: c_int, si_code: c_int, addr: usize) -> Fault {
        let mut thread_name = [0u8; 64];
        thread_name[..4].copy_from_slice(b"main");
        Fault {
            signal,
            si_code,
            addr,
            pc: 0x1_0000_1234,
            fp: 0,
            access: Access::Read,
            fault: true,
            sender: 0,
            pid: 77,
            thread: 4242,
            thread_name,
            stack: (0x7000_0000, 0x7080_0000),
        }
    }

    fn report(f: &Fault, frames: &[usize]) -> serde_json::Value {
        let mut b = Buf::new();
        json(&mut b, f, frames, |at, _| {
            (at >= 0x1_0000_0000).then_some(Module {
                path: b"/Applications/SolidCraft.app/Contents/MacOS/solidcraft",
                base: 0x1_0000_0000,
                symbol: Some((b"_ZN4main17h0123456789abcdefE", 0x1_0000_1200)),
            })
        });
        serde_json::from_slice(b.as_bytes()).unwrap()
    }

    #[test]
    fn a_segfault_says_the_signal_the_address_read_and_the_module() {
        let v = report(&fault(SIGSEGV, 1, 0x10), &[0x1_0000_1234, 0x1_0000_2000, 0x5000]);
        assert_eq!(v["signal"], "SIGSEGV");
        assert_eq!(v["code"], "SIGSEGV");
        assert_eq!(v["name"], "segmentation fault");
        assert_eq!(v["detail"], "read of 0x0000000000000010 (SEGV_MAPERR)");
        assert_eq!(v["address"], "0x0000000100001234");
        assert_eq!(v["module"], "/Applications/SolidCraft.app/Contents/MacOS/solidcraft");
        assert_eq!(v["offset"], "0x1234");
        assert_eq!(v["thread"], 4242);
        assert_eq!(v["thread_name"], "main");
        assert_eq!(v["pid"], 77);
        let frames: Vec<&str> = v["frames"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
        assert_eq!(
            frames,
            ["solidcraft+0x1234 _ZN4main17h0123456789abcdefE+0x34", "solidcraft+0x2000 _ZN4main17h0123456789abcdefE+0xE00", "0x0000000000005000"]
        );
    }

    #[test]
    fn a_fault_below_the_stack_is_a_stack_overflow() {
        let mut f = fault(SIGBUS, 2, 0x7000_0000 - 0x100);
        f.access = Access::Write;
        let v = report(&f, &[]);
        assert_eq!(v["name"], "stack overflow");
        assert_eq!(v["code"], "SIGBUS");
        assert_eq!(v["detail"], "write to 0x000000006FFFFF00, below the thread's stack (BUS_ADRERR)");
        // Far below it, or inside it, it is not.
        assert_eq!(report(&fault(SIGBUS, 2, 0x10), &[])["name"], "bus error");
        assert_eq!(report(&fault(SIGSEGV, 2, 0x7040_0000), &[])["name"], "segmentation fault");
        // Nor without knowing the stack.
        let mut f = fault(SIGSEGV, 1, 0x7000_0000 - 0x100);
        f.stack = (0, 0);
        assert_eq!(report(&f, &[])["name"], "segmentation fault");
    }

    #[test]
    fn an_abort_or_a_signal_sent_by_another_process_has_no_address_detail() {
        let mut f = fault(SIGABRT, 0, 0);
        f.fault = false;
        f.sender = 77;
        let v = report(&f, &[]);
        assert_eq!(v["signal"], "SIGABRT");
        assert_eq!(v["name"], "abort");
        assert!(v.get("detail").is_none(), "{v}");
        let mut f = fault(SIGSEGV, 0x10001, 0);
        f.fault = false;
        f.sender = 501;
        assert_eq!(report(&f, &[])["detail"], "sent by process 501");
        // `kill` without a sender, or `raise`.
        f.sender = 0;
        assert_eq!(report(&f, &[])["detail"], "sent with kill or raise, not a fault");
        // A trap names the instruction.
        assert_eq!(report(&fault(SIGTRAP, 1, 0x1_0000_1234), &[])["detail"], "at 0x0000000100001234");
    }

    #[test]
    fn only_a_fault_runs_again_when_the_handler_returns() {
        // Measured on an M5 Pro, macOS 27: a read of address 16, then `kill -SEGV` while the
        // thread waits in a system call. Both have si_code 2.
        assert!(arm64_fault(SIGSEGV, 2, 0x10, 0x9200_0046, 0x10));
        assert!(!arm64_fault(SIGSEGV, 2, 0, 0x5600_0080, 0));
        assert!(!arm64_fault(SIGBUS, 1, 0, 0x5600_0080, 0));
        // A data abort long past, while the signal names no address.
        assert!(!arm64_fault(SIGSEGV, 2, 0, 0x9200_0046, 0x10));
        // A trap instruction (BRK) or an undefined one; but not a system call.
        assert!(arm64_fault(SIGTRAP, 1, 0x1000, 0xF200_0001, 0));
        assert!(arm64_fault(SIGILL, 1, 0x1000, 0x0200_0000, 0));
        assert!(!arm64_fault(SIGILL, 1, 0x1000, 0x5600_0080, 0));
        // Sent: SI_USER, no code, or an abort.
        assert!(!arm64_fault(SIGSEGV, 0x10001, 0x10, 0x9200_0046, 0x10));
        assert!(!arm64_fault(SIGTRAP, 0, 0, 0xF200_0001, 0));
        assert!(!arm64_fault(SIGABRT, 1, 0, 0x5600_0080, 0));
        // x86_64: a page fault at its address, a general protection fault, ud2, int3.
        assert!(x86_fault(SIGSEGV, 1, 0x10, 14, 0x10));
        assert!(!x86_fault(SIGSEGV, 1, 0, 14, 0x10));
        assert!(x86_fault(SIGSEGV, 1, 0, 13, 0));
        assert!(x86_fault(SIGILL, 1, 0x1000, 6, 0));
        assert!(x86_fault(SIGTRAP, 1, 0x1000, 3, 0));
        assert!(!x86_fault(SIGILL, 1, 0x1000, 14, 0));
    }

    #[test]
    fn names_with_quotes_control_characters_or_bad_utf8_stay_valid_json() {
        let mut f = fault(SIGILL, 1, 0x1_0000_1234);
        f.thread_name = [0; 64];
        f.thread_name[..7].copy_from_slice(b"a\"b\\\n\xFFc");
        let v = report(&f, &[]);
        assert_eq!(v["thread_name"], "a\"b\\??c");
    }

    #[test]
    fn a_report_with_long_names_still_closes_its_json() {
        let long: &'static [u8] = Box::leak(vec![b'x'; 4000].into_boxed_slice());
        let f = fault(SIGSEGV, 1, 0x10);
        let mut b = Buf::new();
        json(&mut b, &f, &[0x1_0000_1234; MAX_FRAMES], |_, _| Some(Module { path: long, base: 0x1_0000_0000, symbol: Some((long, 0x1_0000_0000)) }));
        let v: serde_json::Value = serde_json::from_slice(b.as_bytes()).unwrap();
        assert_eq!(v["module"].as_str().unwrap().len(), 300);
        let frames = v["frames"].as_array().unwrap();
        assert!(!frames.is_empty() && frames.len() <= MAX_FRAMES, "{}", frames.len());
    }

    #[test]
    fn the_frame_walk_stays_on_the_stack_and_goes_up() {
        let stack = (0x1000, 0x2000);
        // Three frames: 0x1100 -> 0x1200 -> 0x1300 -> 0 (end).
        let memory = |at: usize| match at {
            0x1100 => Some(0x1200),
            0x1108 => Some(0xA),
            0x1200 => Some(0x1300),
            0x1208 => Some(0xB),
            0x1300 => Some(0),
            0x1308 => Some(0xC),
            _ => None,
        };
        let mut out = [0; MAX_FRAMES];
        let n = walk(0x9, 0x1100, stack, memory, &mut out);
        assert_eq!(&out[..n], &[0x9, 0xA, 0xB, 0xC]);
        // A frame pointer off the stack, misaligned, or one that doesn't go up ends the walk.
        assert_eq!(walk(0x9, 0x3000, stack, memory, &mut out), 1);
        assert_eq!(walk(0x9, 0x1101, stack, memory, &mut out), 1);
        let looping = |at: usize| if at.is_multiple_of(16) { Some(at) } else { Some(0xA) };
        assert_eq!(walk(0x9, 0x1100, stack, looping, &mut out), 2);
        // A chain that never ends stops at MAX_FRAMES.
        let endless = |at: usize| if at.is_multiple_of(16) { Some(at + 16) } else { Some(0xA) };
        assert_eq!(walk(0x9, 0x1000, stack, endless, &mut out), MAX_FRAMES);
    }

    /// The child half: install crash reporting and the handler, then crash the way
    /// `SOLIDCRAFT_NATIVE_CRASH_FAULT` says.
    #[test]
    fn child() {
        let Some(dir) = std::env::var_os("SOLIDCRAFT_NATIVE_CRASH_CHILD") else { return };
        let build = solidcraft_engine::crash::Build { version: "9.9.9", commit: "abc1234", built: "2026-10-25" };
        let fresh = solidcraft_engine::crash::install(PathBuf::from(dir), build);
        println!("FRESH {}", serde_json::to_string(&fresh).unwrap_or_default());
        let Some(fault) = std::env::var_os("SOLIDCRAFT_NATIVE_CRASH_FAULT") else {
            solidcraft_engine::crash::end_session();
            return;
        };
        super::install();
        solidcraft_engine::crash::phase("device creation");
        match fault.to_str() {
            Some("abort") => std::process::abort(),
            Some("overflow") => println!("unreachable {}", recurse(0)),
            Some("raise") => {
                // SAFETY: none needed; this is the crash under test.
                unsafe { libc::raise(SIGSEGV) };
                println!("survived");
            }
            _ => segfault(),
        }
    }

    /// Runs out of stack.
    #[inline(never)]
    fn recurse(n: u64) -> u64 {
        let a = std::hint::black_box([n; 64]);
        if n == u64::MAX { 0 } else { recurse(n + 1).wrapping_add(a[0]) }
    }

    /// The crash under test: a read of address 16, as a null struct pointer's field would be.
    #[inline(never)]
    fn segfault() {
        // SAFETY: none; this is the crash under test.
        let v = unsafe { std::ptr::read_volatile(std::hint::black_box(16usize) as *const u64) };
        println!("unreachable {v}");
    }

    fn run(dir: &Path, fault: Option<&str>) -> std::process::Output {
        let mut c = std::process::Command::new(std::env::current_exe().unwrap());
        c.args(["--exact", "native_crash_macos::tests::child", "--test-threads=1", "--nocapture"]).env("SOLIDCRAFT_NATIVE_CRASH_CHILD", dir);
        if let Some(f) = fault {
            c.env("SOLIDCRAFT_NATIVE_CRASH_FAULT", f);
        }
        c.output().unwrap()
    }

    /// Crash a child the way `fault` says; returns what the handler wrote and the next start's report.
    fn crash(name: &str, fault: &str, signal: c_int) -> (serde_json::Value, serde_json::Value, String) {
        let dir = std::env::temp_dir().join(format!("solidcraft-native-crash-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let out = run(&dir, Some(fault));
        assert_eq!(out.status.signal(), Some(signal), "the child died of the signal itself: {out:?}");
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        let info =
            std::fs::read_dir(dir.join("sessions")).unwrap().flatten().map(|e| e.path()).find(|p| p.to_string_lossy().ends_with(".native.json"));
        let info = info.expect("the handler wrote what it caught");
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(info).unwrap()).unwrap();
        // The next start reports it.
        let out = run(&dir, None);
        assert!(out.status.success(), "{out:?}");
        let reports = solidcraft_engine::crash::list(&dir);
        assert_eq!(reports.len(), 1, "{reports:?}");
        let r = solidcraft_engine::crash::read(&dir, &reports[0].id).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        (v, r, stderr)
    }

    #[test]
    fn a_segfault_is_reported_with_its_signal_module_frames_and_step() {
        let (v, r, _) = crash("segv", "segv", SIGSEGV);
        assert_eq!(v["signal"], "SIGSEGV", "{v}");
        // Address 16 is in __PAGEZERO, mapped without access: SEGV_ACCERR.
        assert_eq!(v["detail"], "read of 0x0000000000000010 (SEGV_ACCERR)", "{v}");
        assert!(v["module"].as_str().unwrap().contains("solidcraft"), "the fault is in the test binary: {v}");
        let frames = v["frames"].as_array().unwrap();
        assert!(frames.len() >= 3, "{v}");
        assert!(frames.iter().any(|f| f.as_str().unwrap().contains("child")), "the walk reaches the test that crashed: {v}");
        let message = r["message"].as_str().unwrap();
        assert!(message.contains("segmentation fault (SIGSEGV) in solidcraft"), "{r}");
        assert!(message.contains("during startup (device creation"), "{r}");
        assert_eq!(r["kind"], "unclean_exit");
        assert_eq!(r["native"]["signal"], "SIGSEGV");
        assert!(r["backtrace"].as_array().unwrap().iter().any(|f| f.as_str().unwrap().contains("segfault")), "{r}");
    }

    #[test]
    fn an_abort_is_reported_as_sigabrt_with_the_code_that_called_it() {
        let (v, r, _) = crash("abrt", "abort", SIGABRT);
        assert_eq!(v["signal"], "SIGABRT", "{v}");
        assert!(v.get("detail").is_none(), "{v}");
        let frames = v["frames"].as_array().unwrap();
        assert!(frames.iter().any(|f| f.as_str().unwrap().contains("abort")), "{v}");
        assert!(r["message"].as_str().unwrap().contains("abort (SIGABRT)"), "{r}");
    }

    #[test]
    fn a_stack_overflow_is_recorded_and_the_standard_library_still_says_so() {
        // The fault on the guard page is recorded on the alternate stack; the standard library's
        // handler, called after ours, prints its message and aborts.
        let (v, r, stderr) = crash("overflow", "overflow", SIGABRT);
        assert!(matches!(v["signal"].as_str(), Some("SIGSEGV" | "SIGBUS")), "{v}");
        assert_eq!(v["name"], "stack overflow", "{v}");
        assert!(v["detail"].as_str().unwrap().contains("below the thread's stack"), "{v}");
        assert!(stderr.contains("has overflowed its stack"), "{stderr}");
        assert!(r["message"].as_str().unwrap().contains("stack overflow ("), "{r}");
    }

    #[test]
    fn a_segfault_signal_that_was_only_sent_still_ends_the_app() {
        // The standard library's handler, which comes before ours, returns expecting the fault
        // to happen again; a sent signal never does, so it goes to the default action instead.
        let (v, r, _) = crash("raise", "raise", SIGSEGV);
        assert_eq!(v["detail"], "sent with kill or raise, not a fault", "{v}");
        assert!(r["message"].as_str().unwrap().contains("(SIGSEGV)"), "{r}");
    }
}
