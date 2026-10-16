//! FFI wrappers for unit `topology.database_io`.
//!
//! Shim: `harness/cpp/shim/topology_database_io.cpp`.
//! Upstream: `source/SMLib/src/SmDatabaseIO.cpp` (`SmDatabaseIO`, `SmDatabaseIOFile`,
//! `SmDatabaseIOStream` read side) plus the `std::istream` extraction ASCII readers do
//! through `GetInStreamPtr()`.
//!
//! | Rust    | shim                          | upstream                                  |
//! |---------|-------------------------------|-------------------------------------------|
//! | [`run`] | `sc_topology_database_io_run` | a script of `SmDatabaseIO` read calls     |

use std::ffi::CString;

use crate::{check, FfiError, SC_ERR_BUFFER};

mod sys {
    extern "C" {
        pub fn sc_topology_database_io_run(
            mode: i32,
            data: *const u8,
            data_len: i64,
            path: *const std::ffi::c_char,
            file_type: i32,
            word_cap: i64,
            ops: *const i64,
            n_ops: i64,
            open_status: *mut i64,
            records: *mut i64,
            ints: *mut i64,
            ints_cap: i64,
            ints_len: *mut i64,
            dbls: *mut f64,
            dbls_cap: i64,
            dbls_len: *mut i64,
        ) -> i32;
    }
}

/// Script op codes (must match the shim's `enum Op`).
pub mod op {
    /// `[file_type, byte_swap, version, is_big_endian]`.
    pub const INFO: i64 = 0;
    /// `SetVersionNum(arg)`.
    pub const SET_VERSION: i64 = 1;
    /// `SetByteSwap(arg != 0)`.
    pub const SET_BYTE_SWAP: i64 = 2;
    /// `BeginReading()`.
    pub const BEGIN_READING: i64 = 3;
    /// `EndReading()`.
    pub const END_READING: i64 = 4;
    /// `ReadLong(long&)`.
    pub const READ_LONG: i64 = 5;
    /// `ReadLong(ULONG&)`.
    pub const READ_ULONG: i64 = 6;
    /// `ReadLongs(p, arg)`.
    pub const READ_LONGS: i64 = 7;
    /// `ReadDouble`.
    pub const READ_DOUBLE: i64 = 8;
    /// `ReadDoubles(p, arg)`.
    pub const READ_DOUBLES: i64 = 9;
    /// `ReadBoolean`.
    pub const READ_BOOLEAN: i64 = 10;
    /// `ReadBooleans(p, arg)`.
    pub const READ_BOOLEANS: i64 = 11;
    /// `ReadShort`.
    pub const READ_SHORT: i64 = 12;
    /// `ReadChar`.
    pub const READ_CHAR: i64 = 13;
    /// `ReadType(t, arg != 0 ? &dim : NULL)`.
    pub const READ_TYPE: i64 = 14;
    /// `GoToNextLine()`.
    pub const GO_TO_NEXT_LINE: i64 = 15;
    /// `is >> char_buffer`.
    pub const EXTRACT_WORD: i64 = 16;
    /// `is >> long`.
    pub const EXTRACT_LONG: i64 = 17;
    /// `is >> unsigned long`.
    pub const EXTRACT_ULONG: i64 = 18;
    /// `is >> int`.
    pub const EXTRACT_INT: i64 = 19;
    /// `is >> short`.
    pub const EXTRACT_SHORT: i64 = 20;
    /// `is >> double`.
    pub const EXTRACT_DOUBLE: i64 = 21;
    /// `is.get()`.
    pub const GET: i64 = 22;
    /// `is.unget()`.
    pub const UNGET: i64 = 23;
    /// The `ByteSwap*` overloads on `arg` (as integer and as double bits).
    pub const SWAP: i64 = 24;
    /// `is.clear()`.
    pub const CLEAR: i64 = 25;
}

/// Largest count accepted for the array reads (`ReadLongs`, ...).
pub const MAX_COUNT: i64 = 1 << 20;

/// How the reader is built before the script runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source<'a> {
    /// `SmDatabaseIOStream(file_type, &istringstream(data))`.
    Bytes(&'a [u8]),
    /// `SmDatabaseIOFile::OpenFileForRead(path, file_type)`.
    File(&'a str),
    /// `SmDatabaseIO()` (no stream).
    Default,
    /// `SmDatabaseIO(file_type)` (no stream).
    FileTypeOnly,
}

/// Per-op record of [`run`].
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    /// Upstream status (`0` ok), or `-2` for a binary read that left the stream failed.
    pub status: i64,
    /// `rdstate()` after the op, `-1` without a stream.
    pub state: i64,
    /// Read position after the op, `-1` without a stream.
    pub position: i64,
    /// Integers produced.
    pub ints: Vec<i64>,
    /// Doubles produced.
    pub dbls: Vec<f64>,
}

/// Result of [`run`].
#[derive(Debug, Clone, PartialEq)]
pub struct Script {
    /// `OpenFileForRead` status (`0` for other sources).
    pub open_status: i64,
    /// One record per op.
    pub records: Vec<Record>,
}

/// Runs `ops` (`(op, arg)` pairs, codes in [`op`]) against a reader of `file_type`
/// built from `source`.
pub fn run(source: &Source<'_>, file_type: i32, ops: &[(i64, i64)]) -> Result<Script, FfiError> {
    if !(0..=3).contains(&file_type) {
        return Err(FfiError::InvalidArgument(format!("file_type {file_type}")));
    }
    let (mode, data, path, word_cap): (i32, &[u8], Option<CString>, usize) = match source {
        Source::Bytes(d) => (0, d, None, d.len()),
        Source::File(p) => {
            let len = std::fs::metadata(p).map_or(0, |m| m.len() as usize);
            let c = CString::new(*p).map_err(|_| FfiError::InvalidArgument("path".into()))?;
            (1, &[], Some(c), len)
        }
        Source::Default => (2, &[], None, 0),
        Source::FileTypeOnly => (3, &[], None, 0),
    };
    let has_stream = matches!(source, Source::Bytes(_) | Source::File(_));
    let mut ints_cap = 0usize;
    let mut dbls_cap = 0usize;
    for &(code, arg) in ops {
        if !(op::INFO..=op::CLEAR).contains(&code) {
            return Err(FfiError::InvalidArgument(format!("op {code}")));
        }
        let is_array = matches!(code, op::READ_LONGS | op::READ_DOUBLES | op::READ_BOOLEANS);
        if is_array && !(0..=MAX_COUNT).contains(&arg) {
            return Err(FfiError::InvalidArgument(format!("count {arg}")));
        }
        // Extraction and ASCII ReadType dereference the stream unconditionally.
        let derefs = (op::EXTRACT_WORD..=op::UNGET).contains(&code)
            || code == op::CLEAR
            || (code == op::READ_TYPE && file_type == 0);
        if derefs && !has_stream {
            return Err(FfiError::InvalidArgument("op needs a stream".into()));
        }
        ints_cap += if is_array { arg as usize } else { 0 } + word_cap + 8;
        dbls_cap += if is_array { arg as usize } else { 0 } + 2;
    }
    let n_ops = i64::try_from(ops.len()).map_err(|_| FfiError::InvalidArgument("ops".into()))?;
    let flat: Vec<i64> = ops.iter().flat_map(|&(o, a)| [o, a]).collect();
    // Start small and retry once at the exact size the shim reports: the worst-case
    // bounds above can reach tens of GB for generated scripts, which some machines
    // refuse to reserve even though almost none of it is used.
    let mut caps = (ints_cap.clamp(1, 1 << 16), dbls_cap.clamp(1, 1 << 16));
    let mut attempt = 0;
    let (st, records, ints, dbls, ints_len, dbls_len, open_status) = loop {
        let mut records = vec![0i64; 5 * ops.len()];
        let mut ints = vec![0i64; caps.0];
        let mut dbls = vec![0f64; caps.1];
        let (mut ints_len, mut dbls_len, mut open_status) = (0i64, 0i64, 0i64);
        // SAFETY: `data` is valid for `data.len()` reads and `path` (when given) is a
        // NUL-terminated string; `flat` holds 2*n_ops values; `records` has 5*n_ops
        // writable slots; `ints`/`dbls` have the capacities passed (the shim checks
        // them before copying); the length/status out-params are valid i64s. Array
        // counts are bounded by MAX_COUNT, ops that dereference the stream only run
        // when there is one, and word extractions get a buffer of `word_cap` (the data
        // or file length) + 2 bytes inside the shim. Nothing is retained after the call.
        let st = unsafe {
            sys::sc_topology_database_io_run(
                mode,
                data.as_ptr(),
                data.len() as i64,
                path.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()),
                file_type,
                word_cap as i64,
                flat.as_ptr(),
                n_ops,
                &raw mut open_status,
                records.as_mut_ptr(),
                ints.as_mut_ptr(),
                ints.len() as i64,
                &raw mut ints_len,
                dbls.as_mut_ptr(),
                dbls.len() as i64,
                &raw mut dbls_len,
            )
        };
        let need = (
            usize::try_from(ints_len).unwrap_or(0),
            usize::try_from(dbls_len).unwrap_or(0),
        );
        if st == SC_ERR_BUFFER
            && attempt == 0
            && need.0 <= ints_cap.max(1)
            && need.1 <= dbls_cap.max(1)
        {
            caps = (need.0.max(1), need.1.max(1));
            attempt += 1;
            continue;
        }
        break (st, records, ints, dbls, ints_len, dbls_len, open_status);
    };
    if st == SC_ERR_BUFFER {
        return Err(FfiError::Status(st));
    }
    check(st)?;
    let mut ints_it = ints.into_iter().take(ints_len.max(0) as usize);
    let mut dbls_it = dbls.into_iter().take(dbls_len.max(0) as usize);
    let records = records
        .as_chunks::<5>()
        .0
        .iter()
        .map(|&[status, state, position, n_ints, n_dbls]| Record {
            status,
            state,
            position,
            ints: ints_it.by_ref().take(n_ints.max(0) as usize).collect(),
            dbls: dbls_it.by_ref().take(n_dbls.max(0) as usize).collect(),
        })
        .collect();
    Ok(Script {
        open_status,
        records,
    })
}
