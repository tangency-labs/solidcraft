//! The interpreter scx scripts run in, and the worker process that holds it.
//!
//! Scripts are model-written Python, so they are run by Monty (pydantic's Python interpreter
//! written in Rust, built for code from AI models), never by a Python installation. Monty has no
//! way to reach the host by itself: opening a file, listing a folder, reading the environment or
//! the clock suspends the script and asks this host, which refuses ([`refuse_os_call`]). The one
//! thing a script can reach is `_call(tool, arguments_json)`, a SolidCraft MCP tool on the
//! allow-list in `super::HOST_TOOLS`, answered by the server as if the agent had called the tool
//! itself (same path scope, same checks).
//!
//! `solidcraft-cli mcp --scx` runs the interpreter in a child process (`solidcraft-cli
//! scx-worker`, [`worker`]), one per script, capped in memory by the operating system and in time
//! by the server ([`in_worker`]): a script that exhausts memory or crashes the interpreter ends
//! only that child. [`execute`] runs the same steps in this process, for tests and embedders that
//! accept that risk.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use monty::{MontyRepl, ReplProgress};
use monty_types::{
    CompileOptions, ExcType, ExtFunctionResult, MontyException, MontyObject, NameLookupResult, PrintWriter, ResourceLimits, ResourceTracker,
};
use serde_json::{Value, json};

/// The vocabulary a script can call (box, sketch, fillet, part, measure...), run before it.
const PRELUDE: &str = include_str!("prelude.py");

/// Interpreter time one step of a run may take (the script, or the summary after it), not counting
/// the time SolidCraft spends on the tool calls it makes.
pub const STEP_SECONDS: u64 = 30;

/// The memory a worker may use, in bytes (`solidcraft-cli scx-worker` asks the OS for this cap).
pub const WORKER_MEMORY: usize = 1024 * 1024 * 1024;

/// What a script may print before printing raises MemoryError.
const MAX_PRINTED: usize = 256 * 1024;

/// What of a script's printing the reply carries; the rest is counted.
const SHOWN_PRINTED: usize = 8000;

/// The subcommand of `solidcraft-cli` that runs [`worker`].
pub const WORKER_COMMAND: &str = "scx-worker";

/// What to run.
#[derive(Clone, Debug, PartialEq)]
pub enum Job {
    /// The agent's script; `fresh` (true or a name) starts a new design first.
    Run { code: String, fresh: Value },
    /// The outline of the design (the `design` tool).
    Design,
}

impl Job {
    fn to_json(&self) -> Value {
        match self {
            Job::Run { code, fresh } => json!({"job": "run", "code": code, "new": fresh}),
            Job::Design => json!({"job": "design"}),
        }
    }

    fn from_json(v: &Value) -> Result<Job, String> {
        match v["job"].as_str() {
            Some("run") => Ok(Job::Run { code: v["code"].as_str().ok_or("run: no code")?.to_owned(), fresh: v["new"].clone() }),
            Some("design") => Ok(Job::Design),
            _ => Err("unknown job".into()),
        }
    }
}

/// The reply text, and whether the script ended in an error.
#[derive(Debug, PartialEq)]
pub struct Outcome {
    pub text: String,
    pub error: bool,
}

/// Calls a SolidCraft tool for the script: its name and JSON arguments in, its text out, or its
/// error.
pub type Host<'a> = dyn FnMut(&str, &str) -> Result<String, String> + 'a;

/// Runs `job` in this process: the prelude, then the script, then the summary.
pub fn execute(job: &Job, host: &mut Host<'_>) -> Outcome {
    let limits = ResourceLimits { max_feed_duration: Some(Duration::from_secs(STEP_SECONDS)), ..ResourceLimits::default() };
    let repl = MontyRepl::new("script.py", ResourceTracker::new(limits), CompileOptions::default());
    let mut printed = String::new();
    let (repl, r) = feed(repl, PRELUDE, vec![], host, &mut printed);
    let Ok(_) = r else { return internal(r) };
    match job {
        Job::Design => {
            let (_, r) = feed(repl, "_design()", vec![], host, &mut printed);
            match r {
                Ok(v) => Outcome { text: v.as_ref().as_str().unwrap_or_default().to_owned(), error: false },
                Err(e) => Outcome { text: e.summary(), error: true },
            }
        }
        Job::Run { code, fresh } => {
            let fresh = match fresh {
                Value::String(s) => MontyObject::string(s.clone()),
                Value::Bool(b) => MontyObject::bool(*b),
                _ => MontyObject::none(),
            };
            let (repl, r) = feed(repl, "_begin(_fresh)", vec![("_fresh".into(), fresh)], host, &mut printed);
            if let Err(e) = r {
                return Outcome { text: format!("error before the script ran: {}", e.summary()), error: true };
            }
            printed.clear();
            let (repl, r) = feed(repl, code, vec![], host, &mut printed);
            let printed = clip(&printed);
            let (failed, inputs) = match r {
                Ok(last) => (false, end_inputs(&printed, None, last)),
                Err(e) => (true, end_inputs(&printed, Some(&e), MontyObject::none())),
            };
            let mut ignored = String::new();
            let (repl, r) = feed(repl, END, inputs, host, &mut ignored);
            // A last value the summary cannot take back (an object of a class the script made) is
            // left out rather than losing the summary.
            let r = match r {
                Err(_) if !failed => feed(repl, END, end_inputs(&printed, None, MontyObject::none()), host, &mut ignored).1,
                r => r,
            };
            match r {
                Ok(v) => Outcome { text: v.as_ref().as_str().unwrap_or_default().to_owned(), error: failed },
                Err(e) => Outcome { text: format!("{printed}\nerror while summarising the run: {}", e.summary()), error: true },
            }
        }
    }
}

/// `printed`, cut to [`SHOWN_PRINTED`] bytes (at a character boundary) with a count of the rest.
fn clip(printed: &str) -> String {
    if printed.len() <= SHOWN_PRINTED {
        return printed.to_owned();
    }
    let mut cut = SHOWN_PRINTED;
    while !printed.is_char_boundary(cut) {
        cut -= 1;
    }
    format!(
        "{}
... ({} more characters printed)",
        &printed[..cut],
        printed[cut..].chars().count()
    )
}

const END: &str = "_end(_printed, _etype, _emsg, _line, _last)";

/// The arguments of `_end`: what the script printed, its error (type, message, line in the
/// script) and its last value.
fn end_inputs(printed: &str, e: Option<&MontyException>, last: MontyObject) -> Vec<(String, MontyObject)> {
    let (etype, emsg, line) = match e {
        None => (MontyObject::none(), MontyObject::none(), MontyObject::none()),
        Some(e) => {
            // The deepest frame in the script itself: the prelude's frames are not the agent's
            // lines.
            let prelude = "<python-input-0>";
            let line = e.traceback().iter().rev().find(|f| f.filename != prelude).map(|f| i64::from(f.start.line));
            let ty: &'static str = e.exc_type().into();
            (
                MontyObject::string(ty),
                MontyObject::string(e.message().unwrap_or_default()),
                line.map_or_else(|| MontyObject::string("?"), MontyObject::int),
            )
        }
    };
    vec![
        ("_printed".into(), MontyObject::string(printed)),
        ("_etype".into(), etype),
        ("_emsg".into(), emsg),
        ("_line".into(), line),
        ("_last".into(), last),
    ]
}

fn internal(r: Result<MontyObject, MontyException>) -> Outcome {
    let why = r.err().map(|e| e.summary()).unwrap_or_default();
    Outcome { text: format!("scx: the vocabulary failed to load: {why}"), error: true }
}

/// Feeds one snippet to the interpreter and answers what it asks until it ends.
fn feed(
    repl: MontyRepl,
    code: &str,
    inputs: Vec<(String, MontyObject)>,
    host: &mut Host<'_>,
    printed: &mut String,
) -> (MontyRepl, Result<MontyObject, MontyException>) {
    let mut progress = match repl.feed_start(code, inputs, PrintWriter::CollectString(printed, Some(MAX_PRINTED))) {
        Ok(p) => p,
        Err(e) => return (e.repl, Err(e.error)),
    };
    loop {
        let next = match progress {
            ReplProgress::Complete { repl, value } => return (repl, Ok(value)),
            ReplProgress::FunctionCall(call) => {
                let result = host_call(&call.function_name, call.args.args().map(|a| a.as_str().map(str::to_owned)).collect(), host);
                call.resume(result, PrintWriter::CollectString(printed, Some(MAX_PRINTED)))
            }
            ReplProgress::NameLookup(lookup) => {
                let r =
                    if lookup.name == "_call" { NameLookupResult::Value(MontyObject::function("_call", None)) } else { NameLookupResult::Undefined };
                lookup.resume(r, PrintWriter::CollectString(printed, Some(MAX_PRINTED)))
            }
            ReplProgress::OsCall(call) => call.resume(refuse_os_call(), PrintWriter::CollectString(printed, Some(MAX_PRINTED))),
            ReplProgress::ResolveFutures(pending) => {
                // Nothing here ever returns a future, so nothing can be awaited: fail every one.
                let ids: Vec<u32> = pending.pending_call_ids().to_vec();
                let failed = ids.into_iter().map(|id| (id, ExtFunctionResult::Error(not_supported("async code")))).collect();
                pending.resume(failed, PrintWriter::CollectString(printed, Some(MAX_PRINTED)))
            }
        };
        progress = match next {
            Ok(p) => p,
            Err(e) => return (e.repl, Err(e.error)),
        };
    }
}

/// `_call(tool, arguments_json)`: the host answers; anything else a script calls that the
/// interpreter does not know is refused.
fn host_call(name: &str, args: Vec<Option<String>>, host: &mut Host<'_>) -> ExtFunctionResult {
    match (name, args.as_slice()) {
        ("_call", [Some(tool), Some(a)]) => match host(tool, a) {
            Ok(text) => ExtFunctionResult::Return(MontyObject::string(text)),
            Err(e) => ExtFunctionResult::Error(MontyException::new(ExcType::RuntimeError, Some(e))),
        },
        _ => ExtFunctionResult::Error(not_supported(name)),
    }
}

fn not_supported(what: &str) -> MontyException {
    MontyException::new(ExcType::RuntimeError, Some(format!("{what} is not available in SolidCraft scripts")))
}

/// Files, folders, the network, the environment, the clock and processes: never.
fn refuse_os_call() -> ExtFunctionResult {
    ExtFunctionResult::Error(MontyException::new(
        ExcType::PermissionError,
        Some("scripts cannot reach files, folders, the environment or the network; the CAD vocabulary is all they need".into()),
    ))
}

/// The worker's side: one job from `input` (a JSON line), its tool calls asked of the server
/// over `output`/`input`, then the outcome. Run by `solidcraft-cli scx-worker` once the memory cap
/// is in place.
pub fn worker(input: impl Read, mut output: impl Write) -> std::io::Result<()> {
    let mut input = BufReader::new(input);
    let mut line = String::new();
    input.read_line(&mut line)?;
    let job = match serde_json::from_str::<Value>(&line).map_err(|e| e.to_string()).and_then(|v| Job::from_json(&v)) {
        Ok(j) => j,
        Err(e) => {
            writeln!(output, "{}", json!({"fail": e}))?;
            return output.flush();
        }
    };
    let mut io_error = None;
    let outcome = {
        let output = &mut output;
        let input = &mut input;
        let io_error = &mut io_error;
        let mut host = move |tool: &str, args: &str| -> Result<String, String> {
            let mut ask = || -> std::io::Result<Value> {
                writeln!(output, "{}", json!({"call": [tool, args]}))?;
                output.flush()?;
                let mut reply = String::new();
                if input.read_line(&mut reply)? == 0 {
                    return Err(std::io::ErrorKind::UnexpectedEof.into());
                }
                serde_json::from_str(&reply).map_err(std::io::Error::other)
            };
            match ask() {
                Ok(v) => match (v.get("ok").and_then(Value::as_str), v.get("err").and_then(Value::as_str)) {
                    (Some(text), _) => Ok(text.to_owned()),
                    (_, Some(e)) => Err(e.to_owned()),
                    _ => Err("scx: bad reply from the server".into()),
                },
                Err(e) => {
                    let msg = format!("scx: the server went away ({e})");
                    *io_error = Some(e);
                    Err(msg)
                }
            }
        };
        execute(&job, &mut host)
    };
    if let Some(e) = io_error {
        return Err(e);
    }
    writeln!(output, "{}", json!({"done": outcome.text, "error": outcome.error}))?;
    output.flush()
}

/// The server's side: runs `job` in a new `program scx-worker` process and answers its tool calls
/// with `host`. The worker gets [`STEP_SECONDS`] of its own time per step (time spent in `host`
/// does not count) plus a margin, and is killed past that; one that dies (out of memory, a crash)
/// fails only this run.
pub fn in_worker(program: &Path, job: &Job, host: &mut Host<'_>) -> Outcome {
    let mut child = match Command::new(program).arg(WORKER_COMMAND).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
        Ok(c) => c,
        Err(e) => return Outcome { text: format!("scx: could not start the script worker {}: {e}", program.display()), error: true },
    };
    let (Some(mut stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) else {
        let _ = child.kill();
        return Outcome { text: "scx: the script worker has no pipes".into(), error: true };
    };
    let (tx, rx) = mpsc::channel::<Option<String>>();
    std::thread::spawn(move || {
        let mut r = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            match r.read_line(&mut line) {
                Ok(n) if n > 0 => {
                    if tx.send(Some(line)).is_err() {
                        return;
                    }
                }
                _ => {
                    let _ = tx.send(None);
                    return;
                }
            }
        }
    });
    // The last of what the worker said on stderr, for a death to name its cause.
    let err_tail = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.take(64 * 1024).read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).trim().chars().rev().take(400).collect::<Vec<_>>().into_iter().rev().collect::<String>()
    });
    let outcome = converse(&mut stdin, &rx, job, host);
    drop(stdin);
    let outcome = match outcome {
        Ok(o) => o,
        Err(why) => {
            let _ = child.kill();
            let _ = child.wait();
            let tail = err_tail.join().unwrap_or_default();
            let cause = if tail.contains("memory allocation") {
                format!("it ran out of memory (scripts get {} MB)", WORKER_MEMORY / (1024 * 1024))
            } else if tail.is_empty() {
                why
            } else {
                format!("{why}: {tail}")
            };
            return Outcome {
                text: format!("error: the script was stopped: {cause}\n(steps before it stopped were kept; `design` shows what is there)"),
                error: true,
            };
        }
    };
    let _ = child.wait();
    outcome
}

fn converse(stdin: &mut impl Write, rx: &mpsc::Receiver<Option<String>>, job: &Job, host: &mut Host<'_>) -> Result<Outcome, String> {
    let gone = |e: std::io::Error| format!("the worker went away ({e})");
    writeln!(stdin, "{}", job.to_json()).map_err(gone)?;
    stdin.flush().map_err(gone)?;
    // Four steps (vocabulary, start, script, summary), each bounded by the interpreter itself;
    // this is the backstop for a worker that stops answering.
    let mut budget = Duration::from_secs(STEP_SECONDS * 4 + 10);
    loop {
        let waited = Instant::now();
        let line = match rx.recv_timeout(budget) {
            Ok(Some(line)) => line,
            Ok(None) => return Err("the worker ended without an answer".into()),
            Err(mpsc::RecvTimeoutError::Timeout) => return Err(format!("it ran longer than {} s", budget.as_secs())),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err("the worker ended without an answer".into()),
        };
        budget = budget.saturating_sub(waited.elapsed());
        let msg: Value = serde_json::from_str(&line).map_err(|e| format!("the worker said something unreadable ({e})"))?;
        if let Some(text) = msg.get("done").and_then(Value::as_str) {
            return Ok(Outcome { text: text.to_owned(), error: msg["error"] == true });
        }
        if let Some(why) = msg.get("fail").and_then(Value::as_str) {
            return Err(why.to_owned());
        }
        let Some([Value::String(tool), Value::String(args)]) = msg.get("call").and_then(Value::as_array).map(Vec::as_slice) else {
            return Err("the worker sent an unknown message".into());
        };
        let reply = match host(tool, args) {
            Ok(text) => json!({"ok": text}),
            Err(e) => json!({"err": e}),
        };
        writeln!(stdin, "{reply}").map_err(gone)?;
        stdin.flush().map_err(gone)?;
    }
}
