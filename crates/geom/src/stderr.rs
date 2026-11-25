//! A line on stderr that never stops the program: a process another program started (an agent
//! running `solidcraft mcp`, the app that one started) writes into a pipe that program may have
//! closed, and `eprintln!` then panics with "failed printing to stderr" (#10111). A closed stderr
//! is no reason to stop.

use std::io::Write;

/// Write `args` and a line break to stderr, as `eprintln!` does, ignoring a failed write.
pub fn note(args: std::fmt::Arguments<'_>) {
    let mut err = std::io::stderr().lock();
    let _ = err.write_fmt(args).and_then(|()| err.write_all(b"\n"));
}

/// [`note`] with `format!` arguments: `eprintln!` for code that keeps running when stderr is gone.
#[macro_export]
macro_rules! note {
    ($($arg:tt)*) => {
        $crate::stderr::note(format_args!($($arg)*))
    };
}
