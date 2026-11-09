//! Reads a profile written as JSON Lines (`solidcore::profile`, feature `profile`) and
//! prints the scopes with the most self time, and with a second path writes the call paths
//! as folded stacks (`a;b;c <self ns>`, one per line) for flame graph tools such as
//! inferno (`inferno-flamegraph out.folded > out.svg`) or `flamegraph.pl`.
//!
//! ```sh
//! cargo run -p solidcore --example profile_report -- profile.jsonl [out.folded] [top]
//! ```
//!
//! A profile without times (written on wasm32) is ranked and folded by call counts.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)] // a tool

use std::collections::BTreeMap;
use std::io::{BufRead, BufWriter, Write};

struct Scope {
    parent: Option<u64>,
    name: String,
    calls: u64,
    self_ns: Option<u64>,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let input = args
        .next()
        .expect("usage: profile_report <profile.jsonl> [out.folded] [top]");
    let folded = args.next();
    let top: usize = args
        .next()
        .map_or(25, |n| n.parse().expect("top is a count"));

    let file = std::fs::File::open(&input).expect("the profile opens");
    let mut scopes = BTreeMap::new();
    let mut summary = Vec::new();
    for line in std::io::BufReader::new(file).lines() {
        let line = line.expect("the profile reads");
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(&line).expect("each line is JSON");
        match v["type"].as_str() {
            Some("scope") => {
                scopes.insert(
                    v["id"].as_u64().expect("a scope has an id"),
                    Scope {
                        parent: v["parent"].as_u64(),
                        name: v["name"].as_str().unwrap_or("?").to_owned(),
                        calls: v["calls"].as_u64().unwrap_or(0),
                        self_ns: v["self_ns"].as_u64(),
                    },
                );
            }
            Some("summary") => summary.push(v),
            _ => {}
        }
    }

    let timed = summary.iter().any(|e| e["self_ns"].is_u64());
    let key = |e: &serde_json::Value| {
        if timed {
            e["self_ns"].as_u64().unwrap_or(0)
        } else {
            e["count"].as_u64().unwrap_or(0)
        }
    };
    summary.sort_by_key(|e| std::cmp::Reverse(key(e)));
    let all: u64 = summary.iter().map(key).sum();
    println!(
        "{:>12} {:>6} {:>12} {:>10}  name",
        if timed { "self ms" } else { "calls" },
        "%",
        "total ms",
        "count"
    );
    for e in summary.iter().take(top) {
        let ms = |k: &str| {
            e[k].as_u64()
                .map_or_else(|| String::from("-"), |ns| format!("{:.1}", ns as f64 / 1e6))
        };
        let first = if timed {
            ms("self_ns")
        } else {
            key(e).to_string()
        };
        println!(
            "{first:>12} {:>6.1} {:>12} {:>10}  {}",
            100.0 * key(e) as f64 / all.max(1) as f64,
            ms("total_ns"),
            e["count"].as_u64().unwrap_or(0),
            e["name"].as_str().unwrap_or("?"),
        );
    }

    if let Some(path) = folded {
        let mut out = BufWriter::new(std::fs::File::create(&path).expect("the folded file opens"));
        for scope in scopes.values() {
            let value = if timed {
                scope.self_ns.unwrap_or(0)
            } else {
                scope.calls
            };
            if value == 0 {
                continue;
            }
            let mut names = vec![scope.name.as_str()];
            let mut up = scope.parent;
            while let Some(p) = up.and_then(|p| scopes.get(&p)) {
                names.push(p.name.as_str());
                up = p.parent;
            }
            names.reverse();
            writeln!(out, "{} {value}", names.join(";")).expect("the folded file writes");
        }
        out.flush().expect("the folded file writes");
        eprintln!("folded stacks: {path}");
    }
}
