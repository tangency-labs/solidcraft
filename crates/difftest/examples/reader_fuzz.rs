//! Mutation fuzzer for the `.smb` database readers (`BrepData::read_from_file` and the
//! whole-file `topology::brep::p7::read_from_file`).
//!
//! Not part of the difftest gate. Usage (from the repo root):
//!
//! ```sh
//! cargo run --profile difftest -p difftest --example reader_fuzz -- sweep \
//!     --seeds-dir upstream/TestFiles/pt_TestFiles --seeds-dir reader-fuzz-out/corpus-bin \
//!     --cases 2000 --jobs 4 --out reader-fuzz-out
//! cargo run ... --example reader_fuzz -- repro --seed <file> --case <i> [--write out.smb]
//! cargo run ... --example reader_fuzz -- run --input <file> [--binary] [--stage full]
//! cargo run ... --example reader_fuzz -- minimize --input <file> [--binary] --expect <class>
//! ```
//!
//! A seed file is read as binary when its name is under a directory called `corpus-bin`
//! (or with `--binary` for `run`/`minimize`), else as ASCII. Each case is the seed
//! with 1-5 mutations (deterministic from the seed's relative path and the case
//! number). Cases run in child processes (`child` subcommand) under `ulimit -v`, one
//! thread per case with a fixed stack; the parent kills a child that exceeds the
//! per-case time limit and restarts after the dead case. Verdicts: `ok`, `err`,
//! `unported` (fine), `panic`, `oom`, `stack-overflow`, `crash`, `timeout` (bad).
//!
//! Unix-only (child processes under `sh -c 'ulimit -v'`, signal exit statuses,
//! `/proc/self/*`); on other platforms the example builds but exits with status 2.

#[cfg(unix)]
#[path = "reader_fuzz/fuzz.rs"]
mod fuzz;

#[cfg(unix)]
fn main() {
    fuzz::main();
}

#[cfg(not(unix))]
fn main() {
    eprintln!("reader_fuzz is Unix-only");
    std::process::exit(2);
}
