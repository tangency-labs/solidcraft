//! Runner for unit `topology.database_io`: SMLib database stream reading.
//!
//! Ported from upstream `source/SMLib/src/SmDatabaseIO.cpp` (plus libstdc++
//! `std::istream` extraction); checked against its golden. Rust:
//! `solidcore::topology::database_io`.
//!
//! Each case is a *script*: a reader is built from `source` (in-memory bytes, an
//! upstream test file via `OpenFileForRead`, or no stream) and a list of
//! `[op, arg]` steps runs against it. Per step the runner records the status, the
//! stream state (`rdstate`), the read position, and the integers/doubles produced;
//! doubles are also recorded bit for bit (`dbl_bits`; subnormals only there).
//!
//! Normalisation: a binary read that leaves the stream failed reports status `-2`
//! (upstream returns `SM_SUCCESS` with an uninitialised value; Rust returns
//! `SolidError::InvalidInput`). An extraction whose sentry fails stores nothing:
//! its `None` is recorded as the variable's initial sentinel, as upstream reports it. Every run has at least [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::topology::database_io::{
    byte_swap_f64, byte_swap_u16, byte_swap_u32, byte_swap_u64, is_big_endian, DatabaseReader,
    FileType, InputStream,
};
use solidcore::SolidError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Script op codes (upstream's `SmDatabaseIO` calls).
mod op {
    /// `[file_type, byte_swap, version, is_big_endian]`.
    pub(super) const INFO: i64 = 0;
    /// `SetVersionNum(arg)`.
    pub(super) const SET_VERSION: i64 = 1;
    /// `SetByteSwap(arg != 0)`.
    pub(super) const SET_BYTE_SWAP: i64 = 2;
    /// `BeginReading()`.
    pub(super) const BEGIN_READING: i64 = 3;
    /// `EndReading()`.
    pub(super) const END_READING: i64 = 4;
    /// `ReadLong(long&)`.
    pub(super) const READ_LONG: i64 = 5;
    /// `ReadLong(ULONG&)`.
    pub(super) const READ_ULONG: i64 = 6;
    /// `ReadLongs(p, arg)`.
    pub(super) const READ_LONGS: i64 = 7;
    /// `ReadDouble`.
    pub(super) const READ_DOUBLE: i64 = 8;
    /// `ReadDoubles(p, arg)`.
    pub(super) const READ_DOUBLES: i64 = 9;
    /// `ReadBoolean`.
    pub(super) const READ_BOOLEAN: i64 = 10;
    /// `ReadBooleans(p, arg)`.
    pub(super) const READ_BOOLEANS: i64 = 11;
    /// `ReadShort`.
    pub(super) const READ_SHORT: i64 = 12;
    /// `ReadChar`.
    pub(super) const READ_CHAR: i64 = 13;
    /// `ReadType(t, arg != 0 ? &dim : NULL)`.
    pub(super) const READ_TYPE: i64 = 14;
    /// `GoToNextLine()`.
    pub(super) const GO_TO_NEXT_LINE: i64 = 15;
    /// `is >> char_buffer`.
    pub(super) const EXTRACT_WORD: i64 = 16;
    /// `is >> long`.
    pub(super) const EXTRACT_LONG: i64 = 17;
    /// `is >> unsigned long`.
    pub(super) const EXTRACT_ULONG: i64 = 18;
    /// `is >> int`.
    pub(super) const EXTRACT_INT: i64 = 19;
    /// `is >> short`.
    pub(super) const EXTRACT_SHORT: i64 = 20;
    /// `is >> double`.
    pub(super) const EXTRACT_DOUBLE: i64 = 21;
    /// `is.get()`.
    pub(super) const GET: i64 = 22;
    /// `is.unget()`.
    pub(super) const UNGET: i64 = 23;
    /// The `ByteSwap*` overloads on `arg` (as integer and as double bits).
    pub(super) const SWAP: i64 = 24;
    /// `is.clear()`.
    pub(super) const CLEAR: i64 = 25;
}

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `topology.database_io` runner.
pub struct Runner;

const ST_SHORT: i64 = -2;
const SM_ERR_INVALID_INPUT: i64 = 1007;
const SENT_LONG: i64 = -777777;
const SENT_ULONG: u64 = 888888;
const SENT_INT: i32 = -777777;
const SENT_SHORT: i16 = -7777;
const SENT_DOUBLE: f64 = -7777.25;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum SourceKind {
    Bytes,
    File,
    Default,
    FileTypeOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    source: SourceKind,
    /// Text data (`source = bytes`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    /// Raw data (`source = bytes`), used when `text` is absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    data: Option<Vec<u8>>,
    /// File path (`source = file`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    file_type: i32,
    ops: Vec<(String, i64)>,
}

const OPS: &[(&str, i64)] = &[
    ("info", op::INFO),
    ("set_version", op::SET_VERSION),
    ("set_byte_swap", op::SET_BYTE_SWAP),
    ("begin_reading", op::BEGIN_READING),
    ("end_reading", op::END_READING),
    ("read_long", op::READ_LONG),
    ("read_ulong", op::READ_ULONG),
    ("read_longs", op::READ_LONGS),
    ("read_double", op::READ_DOUBLE),
    ("read_doubles", op::READ_DOUBLES),
    ("read_boolean", op::READ_BOOLEAN),
    ("read_booleans", op::READ_BOOLEANS),
    ("read_short", op::READ_SHORT),
    ("read_char", op::READ_CHAR),
    ("read_type", op::READ_TYPE),
    ("go_to_next_line", op::GO_TO_NEXT_LINE),
    ("extract_word", op::EXTRACT_WORD),
    ("extract_long", op::EXTRACT_LONG),
    ("extract_ulong", op::EXTRACT_ULONG),
    ("extract_int", op::EXTRACT_INT),
    ("extract_short", op::EXTRACT_SHORT),
    ("extract_double", op::EXTRACT_DOUBLE),
    ("get", op::GET),
    ("unget", op::UNGET),
    ("swap", op::SWAP),
    ("clear", op::CLEAR),
];

fn op_code(name: &str) -> Result<i64, HarnessError> {
    OPS.iter()
        .find(|(n, _)| *n == name)
        .map(|&(_, c)| c)
        .ok_or_else(|| HarnessError::new(format!("unknown op `{name}`")))
}

/// The `(op, arg)` list of a case. `["loop", n]` ... `["end", 0]` repeats the
/// enclosed ops `n` times (not nested), keeping long file scripts short.
fn script(i: &Inputs) -> Result<Vec<(i64, i64)>, HarnessError> {
    let mut out = Vec::new();
    let mut body: Option<(usize, Vec<(i64, i64)>)> = None;
    for (n, a) in &i.ops {
        match (n.as_str(), body.as_mut()) {
            ("loop", None) => body = Some((usize::try_from(*a).unwrap_or(0), Vec::new())),
            ("end", Some(_)) => {
                if let Some((times, ops)) = body.take() {
                    for _ in 0..times {
                        out.extend_from_slice(&ops);
                    }
                }
            }
            ("loop" | "end", _) => return Err(HarnessError::new("unbalanced loop/end")),
            (name, Some((_, ops))) => ops.push((op_code(name)?, *a)),
            (name, None) => out.push((op_code(name)?, *a)),
        }
    }
    if body.is_some() {
        return Err(HarnessError::new("unterminated loop"));
    }
    Ok(out)
}

fn bytes_of(i: &Inputs) -> Vec<u8> {
    match (&i.text, &i.data) {
        (Some(t), _) => t.as_bytes().to_vec(),
        (None, Some(d)) => d.clone(),
        (None, None) => Vec::new(),
    }
}

/// One op's result, the shape both sides produce.
struct Rec {
    status: i64,
    state: i64,
    position: i64,
    ints: Vec<i64>,
    dbls: Vec<f64>,
}

fn output(open_status: i64, recs: &[Rec]) -> Output {
    let all: Vec<f64> = recs.iter().flat_map(|r| r.dbls.iter().copied()).collect();
    let bits: Vec<i64> = all.iter().map(|d| d.to_bits() as i64).collect();
    // Subnormals are recorded through `dbl_bits` only: a relative change of a subnormal can
    // round back to the same value.
    let dbls: Vec<f64> = all.into_iter().filter(|d| !d.is_subnormal()).collect();
    Output::new()
        .int("open_status", open_status)
        .ints("status", recs.iter().map(|r| r.status).collect::<Vec<_>>())
        .ints("state", recs.iter().map(|r| r.state).collect::<Vec<_>>())
        .ints(
            "position",
            recs.iter().map(|r| r.position).collect::<Vec<_>>(),
        )
        .ints(
            "n_ints",
            recs.iter().map(|r| r.ints.len() as i64).collect::<Vec<_>>(),
        )
        .ints(
            "n_dbls",
            recs.iter().map(|r| r.dbls.len() as i64).collect::<Vec<_>>(),
        )
        .ints(
            "ints",
            recs.iter()
                .flat_map(|r| r.ints.iter().copied())
                .collect::<Vec<_>>(),
        )
        .ints("dbl_bits", bits)
        .values("dbls", dbls)
}

// ---- Rust side ------------------------------------------------------------------

/// Status for a Rust error (see module docs).
fn status_of(e: &SolidError) -> Result<i64, HarnessError> {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => Ok(ST_SHORT),
        up_err if solidcore::error::upstream_status(up_err).is_some() => Ok(i64::from(solidcore::error::upstream_status(up_err).unwrap_or(-1))),
        other => Err(HarnessError::new(format!("unexpected error {other}"))),
    }
}

fn stream(r: &mut DatabaseReader) -> Result<&mut InputStream, HarnessError> {
    r.in_stream_mut()
        .ok_or_else(|| HarnessError::new("op needs a stream"))
}

/// Runs one op on the Rust reader; returns (status, ints, dbls).
fn rust_op(r: &mut DatabaseReader, code: i64, arg: i64) -> Result<Rec, HarnessError> {
    let mut ints = Vec::new();
    let mut dbls = Vec::new();
    let count = usize::try_from(arg).unwrap_or(0);
    let res: solidcore::Result<()> = match code {
        op::INFO => {
            ints.extend([
                i64::from(r.file_type().code()),
                i64::from(r.byte_swap()),
                r.version() as i64,
                i64::from(is_big_endian()),
            ]);
            Ok(())
        }
        op::SET_VERSION => {
            r.set_version(arg as u64);
            Ok(())
        }
        op::SET_BYTE_SWAP => {
            r.set_byte_swap(arg != 0);
            Ok(())
        }
        op::BEGIN_READING => {
            r.begin_reading();
            Ok(())
        }
        op::END_READING => {
            r.end_reading();
            Ok(())
        }
        op::READ_LONG => r.read_long().map(|v| ints.push(v)),
        op::READ_ULONG => r.read_ulong().map(|v| ints.push(v as i64)),
        op::READ_LONGS => r
            .read_longs(count)
            .map(|v| ints.extend(v.into_iter().map(|x| x as i64))),
        op::READ_DOUBLE => r.read_double().map(|v| dbls.push(v)),
        op::READ_DOUBLES => r.read_doubles(count).map(|v| dbls.extend(v)),
        op::READ_BOOLEAN => r.read_boolean().map(|v| ints.push(i64::from(v))),
        op::READ_BOOLEANS => r
            .read_booleans(count)
            .map(|v| ints.extend(v.into_iter().map(i64::from))),
        op::READ_SHORT => r.read_short().map(|v| ints.push(i64::from(v))),
        op::READ_CHAR => r.read_char().map(|v| ints.push(i64::from(v))),
        op::READ_TYPE => r.read_type(arg != 0).map(|h| {
            ints.push(h.type_code.unwrap_or(SENT_LONG));
            if arg != 0 {
                ints.push(h.dim.unwrap_or(SENT_ULONG) as i64);
            }
        }),
        op::GO_TO_NEXT_LINE => {
            r.go_to_next_line();
            Ok(())
        }
        op::EXTRACT_WORD => {
            if let Some(w) = stream(r)?.extract_word() {
                ints.extend(w.into_iter().map(i64::from));
            }
            Ok(())
        }
        op::EXTRACT_LONG => {
            ints.push(stream(r)?.extract_long().unwrap_or(SENT_LONG));
            Ok(())
        }
        op::EXTRACT_ULONG => {
            ints.push(stream(r)?.extract_ulong().unwrap_or(SENT_ULONG) as i64);
            Ok(())
        }
        op::EXTRACT_INT => {
            ints.push(i64::from(stream(r)?.extract_int().unwrap_or(SENT_INT)));
            Ok(())
        }
        op::EXTRACT_SHORT => {
            ints.push(i64::from(stream(r)?.extract_short().unwrap_or(SENT_SHORT)));
            Ok(())
        }
        op::EXTRACT_DOUBLE => {
            dbls.push(stream(r)?.extract_double().unwrap_or(SENT_DOUBLE));
            Ok(())
        }
        op::GET => {
            ints.push(stream(r)?.get().map_or(-1, i64::from));
            Ok(())
        }
        op::UNGET => {
            stream(r)?.unget();
            Ok(())
        }
        op::CLEAR => {
            stream(r)?.clear();
            Ok(())
        }
        op::SWAP => {
            let u = arg as u64;
            let d = f64::from_bits(u);
            ints.extend([
                i64::from(byte_swap_u16(u as u16)),
                i64::from(byte_swap_u32(u as u32)),
                i64::from(byte_swap_u32(u as u32)),
                byte_swap_u64(u) as i64,
                byte_swap_u64(u) as i64,
            ]);
            dbls.extend([byte_swap_f64(d), byte_swap_f64(d)]);
            Ok(())
        }
        other => return Err(HarnessError::new(format!("op {other}"))),
    };
    let status = match res {
        Ok(()) => 0,
        Err(e) => {
            ints.clear();
            dbls.clear();
            status_of(&e)?
        }
    };
    let state = r.in_stream().map_or(-1, |s| i64::from(s.state().bits()));
    let position = r.in_stream().map_or(-1, |s| s.position() as i64);
    Ok(Rec {
        status,
        state,
        position,
        ints,
        dbls,
    })
}

fn run_rust_script(i: &Inputs) -> Result<Output, HarnessError> {
    let ft = FileType::from_code(i.file_type)
        .map_err(|_| HarnessError::new(format!("file_type {}", i.file_type)))?;
    let mut open_status = 0;
    let mut reader = match i.source {
        SourceKind::Bytes => DatabaseReader::with_stream(ft, InputStream::new(bytes_of(i))),
        SourceKind::File => {
            let path = i
                .path
                .as_deref()
                .ok_or_else(|| HarnessError::new("no path"))?;
            // The caller loads the file (the library has no filesystem access); a
            // file that cannot be opened is upstream's SM_ERR_INVALID_INPUT.
            if let Ok(bytes) = std::fs::read(path) {
                let mut r = DatabaseReader::new();
                r.open_for_read(bytes, ft);
                r
            } else {
                open_status = SM_ERR_INVALID_INPUT;
                DatabaseReader::with_file_type(ft)
            }
        }
        SourceKind::Default => DatabaseReader::new(),
        SourceKind::FileTypeOnly => DatabaseReader::with_file_type(ft),
    };
    let recs = script(i)?
        .into_iter()
        .map(|(c, a)| rust_op(&mut reader, c, a))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(output(open_status, &recs))
}

// ---- generator ------------------------------------------------------------------

const ASCII_TOKENS: &[&str] = &[
    "0",
    "00",
    "7",
    "-12",
    "+5",
    "-",
    "+",
    "+-3",
    "123456789",
    "-0",
    "0x1F",
    "99999999999999999999",
    "9223372036854775807",
    "9223372036854775808",
    "-9223372036854775808",
    "-9223372036854775809",
    "18446744073709551615",
    "18446744073709551616",
    "-1",
    "40000",
    "-40000",
    "3000000000",
    "-3000000000",
    "1.5",
    "-0.25",
    "1e3",
    "1E-3",
    "1e",
    "1e+",
    "1e-x",
    ".5",
    "5.",
    ".",
    "-.5",
    "1.2.3",
    "1e999",
    "-1e999",
    "1e-400",
    "4.9e-324",
    "2.2250738585072014e-308",
    "0.1",
    "6.2831853070000000",
    "13.1710542148281780",
    "00012.5e-2",
    "nan",
    "inf",
    "Object",
    "Type",
    "Curve",
    "Dim",
    "SmLine",
    "//PolyBrep",
    "-",
    "abc",
    "x9",
    "9x",
    "1,5",
];

const SEPARATORS: &[&str] = &[
    " ", " ", " ", "  ", "\t", "\n", "\n", "\r\n", "\x0b", "\x0c", "",
];

const ASCII_OPS: &[&str] = &[
    "extract_word",
    "extract_long",
    "extract_ulong",
    "extract_int",
    "extract_short",
    "extract_double",
    "extract_double",
    "go_to_next_line",
    "go_to_next_line",
    "get",
    "unget",
    "info",
    "read_type",
    "clear",
];

const BINARY_OPS: &[&str] = &[
    "read_long",
    "read_ulong",
    "read_longs",
    "read_double",
    "read_doubles",
    "read_boolean",
    "read_booleans",
    "read_short",
    "read_char",
    "read_type",
    "set_byte_swap",
    "set_version",
    "begin_reading",
    "end_reading",
    "info",
    "go_to_next_line",
];

fn pick<'a>(rng: &mut Rng, items: &[&'a str]) -> &'a str {
    rng.choose(items).copied().unwrap_or("")
}

fn gen_ascii(rng: &mut Rng) -> Inputs {
    let mut text = String::new();
    if rng.chance(0.2) {
        text.push_str("Object Type 14010\r\nCurve Dim 2\r\n");
    }
    let n_tokens = rng.below(40) as usize;
    for _ in 0..n_tokens {
        if rng.chance(0.03) {
            // a line longer than SM_MAXSIZE, sometimes exactly at the boundary
            let len = 250 + rng.below(12) as usize;
            text.push_str(&"y".repeat(len));
            text.push('\n');
        }
        text.push_str(pick(rng, ASCII_TOKENS));
        text.push_str(pick(rng, SEPARATORS));
    }
    let n_ops = 1 + rng.below(30) as usize;
    let ops = (0..n_ops)
        .map(|_| {
            let name = pick(rng, ASCII_OPS);
            let arg = if name == "read_type" {
                rng.below(2) as i64
            } else {
                0
            };
            (name.to_string(), arg)
        })
        .collect();
    Inputs {
        source: SourceKind::Bytes,
        text: Some(text),
        data: None,
        path: None,
        file_type: 0,
        ops,
    }
}

fn gen_binary(rng: &mut Rng) -> Inputs {
    let len = rng.below(96) as usize;
    let data: Vec<u8> = (0..len)
        .map(|_| {
            if rng.chance(0.5) {
                // small little-endian values are common in real files
                if rng.chance(0.7) {
                    0
                } else {
                    rng.below(4) as u8
                }
            } else {
                rng.below(256) as u8
            }
        })
        .collect();
    let file_type = 1 + rng.below(3) as i32;
    let n_ops = 1 + rng.below(20) as usize;
    let ops = (0..n_ops)
        .map(|_| {
            let name = pick(rng, BINARY_OPS);
            let arg = match name {
                "read_longs" | "read_doubles" | "read_booleans" => rng.below(5) as i64,
                "read_type" | "set_byte_swap" => rng.below(2) as i64,
                "set_version" => *rng.choose(&[30i64, 33, 39, 40, 41]).unwrap_or(&30),
                _ => 0,
            };
            (name.to_string(), arg)
        })
        .collect();
    Inputs {
        source: SourceKind::Bytes,
        text: None,
        data: Some(data),
        path: None,
        file_type,
        ops,
    }
}

fn gen_swap(rng: &mut Rng) -> Inputs {
    let ops = (0..8)
        .map(|_| ("swap".to_string(), rng.next_u64() as i64))
        .collect();
    Inputs {
        source: SourceKind::Default,
        text: None,
        data: None,
        path: None,
        file_type: 3,
        ops,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.database_io"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let r = rng.unit();
        let inputs = if r < 0.5 {
            gen_ascii(rng)
        } else if r < 0.9 {
            gen_binary(rng)
        } else {
            gen_swap(rng)
        };
        serde_json::to_value(inputs).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        run_rust_script(&case.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WR-47: `ReadBoolean` reads a 4-byte boolean as `ReadBooleans` reads one (upstream's
    /// own byte-swapping path, `SmDatabaseIO.cpp:547`). Every case of the unit that reads
    /// a boolean records the same output with each `read_boolean` step replaced by
    /// `read_booleans 1`: status, stream state, position and value, byte-swapped streams
    /// included.
    #[test]
    fn read_boolean_reads_as_read_booleans_of_one() {
        let ctx = CaseContext {
            root: crate::case::repo_root(),
            seeds: MIN_GENERATED,
        };
        let mut replaced = 0usize;
        let mut swapped_cases = 0usize;
        for case in Runner.cases(&ctx).unwrap() {
            let mut i: Inputs = case.parse().unwrap();
            if !i.ops.iter().any(|(o, _)| o == "read_boolean") {
                continue;
            }
            let want = run_rust_script(&i).unwrap();
            for step in &mut i.ops {
                if step.0 == "read_boolean" {
                    *step = ("read_booleans".to_string(), 1);
                    replaced += 1;
                }
            }
            if i.file_type == 2 || i.ops.iter().any(|(o, a)| o == "set_byte_swap" && *a != 0) {
                swapped_cases += 1;
            }
            let got = run_rust_script(&i).unwrap();
            assert_eq!(got, want, "{}", case.name);
        }
        assert!(replaced > 0 && swapped_cases > 0, "{replaced} {swapped_cases}");
    }
}
