//! Seeded goldens: what every robustness seed builds (`tests/robustness/goldens.json`).
//!
//! Each seed's random design (`fuzz::run`) is recorded: its steps, how it ended (ok, or the
//! kind and start of the error it was refused with) and, for what it left, the bodies' total
//! volume and area (within a relative 1e-6) and their face, edge and shell counts (exact). Any
//! change fails, the better and the worse alike: a seed that used to be refused and now builds
//! is recorded on purpose, with the reason. To record: `ROBUSTNESS_GOLDENS_BLESS="why" cargo
//! test --profile ci -p solidcraft-engine --lib robustness_goldens::` (docs/robustness.md); run
//! it with `cargo test`, whose tests share one process and so one lock on the file, not with
//! nextest. The reason goes into the file's `notes` with the seeds it changed.
//!
//! Each seed is its own test (`seed_00` … `seed_95`), so the runner spreads them over the CI
//! shards and their cores: a few seeds take minutes, and grouped four ways they made one test
//! of half an hour. Pull requests that can change them run every seed but the slowest few
//! (`SLOW_SEEDS` in ci.yml); the merge queue, pushes to main and the nightly run every seed.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::{Value, json};

use crate::fuzz;

/// Seeds 0..SEEDS (about ten minutes for all of them on one core), each with up to this many steps (as `fixed_seeds_run_clean`).
const SEEDS: u64 = 96;
const MAX_STEPS: usize = 8;
const REL: f64 = 1e-6;

/// Tests share the file when they record.
static FILE: Mutex<()> = Mutex::new(());

fn path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/robustness/goldens.json")
}

fn load() -> Value {
    std::fs::read(path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_else(|| json!({}))
}

/// How `got` differs from `want` (None when it matches within the tolerances).
fn differs(want: &Value, got: &Value) -> Option<String> {
    let mut out = Vec::new();
    for (k, g) in got.as_object().into_iter().flatten() {
        let w = &want[k];
        let same = match (w.as_f64(), g.as_f64()) {
            (Some(a), Some(b)) if matches!(k.as_str(), "volume" | "area") => (a - b).abs() <= REL * a.abs().max(b.abs()).max(1.0),
            _ => w == g,
        };
        if !same {
            out.push(format!("{k}: {w} -> {g}"));
        }
    }
    (!out.is_empty()).then(|| out.join("; "))
}

/// Only Windows needs an environment split; other OS policies remain compatible.
#[derive(Clone, Copy, Debug)]
struct Target<'a> {
    os: &'a str,
    env: &'a str,
}

impl<'a> Target<'a> {
    fn current() -> Self {
        // cfg! describes the compilation target, including when cross-compiling.
        let env = if cfg!(target_env = "msvc") {
            "msvc"
        } else if cfg!(target_env = "gnu") {
            "gnu"
        } else {
            "other"
        };
        Self { os: std::env::consts::OS, env }
    }

    fn platform(self) -> Option<&'a str> {
        match (self.os, self.env) {
            ("windows", "msvc") => Some("windows"), // Existing MSVC-verified records.
            ("windows", "gnu") => Some("windows-gnu"),
            ("windows", _) => None,
            (os, _) => Some(os),
        }
    }

    fn recording_platform(self, file: &Value) -> Result<Option<&'a str>, String> {
        let platform = self.platform();
        if self.os == "windows" {
            return platform.map(Some).ok_or_else(|| format!("no recording policy for Windows target_env={}; define one before blessing", self.env));
        }
        Ok(platform.filter(|p| file["platforms"][*p]["seeds"].is_object()))
    }
}

/// An override replaces the entire seed record, never individual fields.
/// The second return value identifies the selected reference in diagnostics.
fn expected<'a, 't>(file: &'a Value, seed: u64, target: Target<'t>) -> (&'a Value, &'t str) {
    let key = seed.to_string();
    if let Some(platform) = target.platform()
        && let Some(record) = file.get("platforms").and_then(|p| p.get(platform)).and_then(|p| p.get("seeds")).and_then(|s| s.get(&key))
    {
        return (record, platform);
    }
    (&file["seeds"][key], "shared")
}

fn difference(file: &Value, seed: u64, got: &Value, target: Target<'_>) -> Option<String> {
    let (want, group) = expected(file, seed, target);
    let change = if want.is_null() { Some("new".to_string()) } else { differs(want, got) };
    change.map(|d| format!("seed {seed} [target_os={}, target_env={}, reference={group}]: {d}", target.os, target.env))
}

/// In-memory recording seam: regression tests never write the real reference file.
fn record(file: &mut Value, seed: u64, got: &Value, reason: &str, target: Target<'_>) -> Result<bool, String> {
    if reason.trim().is_empty() {
        return Err("ROBUSTNESS_GOLDENS_BLESS needs the reason for the change".to_string());
    }
    let platform = target.recording_platform(file)?;
    if difference(file, seed, got, target).is_none() {
        return Ok(false);
    }
    if file.get("about").is_none() {
        file["about"] = json!(
            "What each robustness seed builds (crates/engine/src/tests/robustness_goldens.rs, docs/robustness.md). Record changes with ROBUSTNESS_GOLDENS_BLESS=\"why\"."
        );
    }
    if let Some(platform) = platform {
        file["platforms"][platform]["seeds"][seed.to_string()] = got.clone();
    } else {
        file["seeds"][seed.to_string()] = got.clone();
    }
    let mut notes = file["notes"].as_array().cloned().unwrap_or_default();
    // Coalesce only the same reason AND destination, including shared vs platform.
    // Windows destinations encode the environment, so GNU and MSVC cannot coalesce.
    let destination = platform.map(|p| json!(p));
    match notes.last_mut() {
        Some(last) if last["reason"] == json!(reason) && last.get("platform") == destination.as_ref() => {
            let mut all: Vec<u64> = last["seeds"].as_array().into_iter().flatten().filter_map(Value::as_u64).chain([seed]).collect();
            all.sort_unstable();
            all.dedup();
            last["seeds"] = json!(all);
        }
        _ => {
            let mut note = json!({"reason": reason, "seeds": [seed]});
            if let Some(platform) = platform {
                note["platform"] = json!(platform);
            }
            notes.push(note);
        }
    }
    file["notes"] = Value::Array(notes);
    Ok(true)
}

fn serialize(file: &Value) -> String {
    // Seeds in order, one per line, so a change is a small diff.
    let seeds: BTreeMap<u64, Value> =
        file["seeds"].as_object().into_iter().flatten().filter_map(|(k, v)| Some((k.parse::<u64>().ok()?, v.clone()))).collect();
    let mut s = format!("{{\n \"about\": {},\n \"seeds\": {{\n", file["about"]);
    let n = seeds.len();
    for (i, (k, v)) in seeds.iter().enumerate() {
        s += &format!("  \"{k}\": {}{}\n", v, if i + 1 < n { "," } else { "" });
    }
    let notes: Vec<String> = file["notes"].as_array().into_iter().flatten().map(|n| format!("  {n}")).collect();
    s += " },\n";
    if let Some(platforms) = file.get("platforms") {
        s += &format!(" \"platforms\": {platforms},\n");
    }
    s += &format!(" \"notes\": [\n{}\n ]\n}}\n", notes.join(",\n"));
    s
}

fn check(seed: u64) {
    let got = fuzz::golden(seed, MAX_STEPS);
    let _lock = FILE.lock().unwrap_or_else(|e| e.into_inner());
    let mut file = load();
    let target = Target::current();
    if let Ok(reason) = std::env::var("ROBUSTNESS_GOLDENS_BLESS") {
        if record(&mut file, seed, &got, &reason, target).unwrap_or_else(|e| panic!("{e}")) {
            std::fs::write(path(), serialize(&file)).unwrap_or_else(|e| panic!("{}: {e}", path().display()));
        }
        return;
    }
    let change = difference(&file, seed, &got, target);
    assert!(
        change.is_none(),
        "build differs from tests/robustness/goldens.json (if that is intended, record it with ROBUSTNESS_GOLDENS_BLESS=\"why\" and say why in the pull request):\n{}",
        change.unwrap_or_default()
    );
}

const MSVC: Target<'_> = Target { os: "windows", env: "msvc" };
const GNU: Target<'_> = Target { os: "windows", env: "gnu" };
const LINUX: Target<'_> = Target { os: "linux", env: "gnu" };

fn policy_fixture() -> Value {
    json!({
        "seeds": {"1": {"volume": 10, "faces": 2, "outcome": "ok"}, "2": {"volume": 20}},
        "platforms": {
            "windows": {"seeds": {"1": {"volume": 11, "outcome": "failed"}}, "verification": "MSVC"},
            "macos": {"seeds": {"1": {"volume": 12}}}
        },
        "notes": [{"platform": "windows", "reason": "same reason", "seeds": [1]}]
    })
}

#[test]
fn policy_selects_msvc_and_gnu_overrides_with_shared_fallback() {
    let mut file = policy_fixture();
    assert_eq!(expected(&file, 1, MSVC), (&file["platforms"]["windows"]["seeds"]["1"], "windows"));
    assert_eq!(expected(&file, 1, GNU), (&file["seeds"]["1"], "shared"));
    file["platforms"]["windows-gnu"]["seeds"]["1"] = json!({"volume": 13});
    assert_eq!(expected(&file, 1, GNU), (&file["platforms"]["windows-gnu"]["seeds"]["1"], "windows-gnu"));
    for target in [MSVC, GNU] {
        assert_eq!(expected(&file, 2, target), (&file["seeds"]["2"], "shared"));
    }
    // A platform record is whole, including failure outcomes and missing fields.
    assert!(expected(&file, 1, MSVC).0.get("faces").is_none());
    assert_eq!(expected(&file, 1, MSVC).0["outcome"], "failed");
    assert!(differs(expected(&file, 1, MSVC).0, &file["seeds"]["1"]).is_some());
    assert!(expected(&file, 1, GNU).0.get("faces").is_none());
    assert!(expected(&file, 99, GNU).0.is_null());
}

#[test]
fn policy_preserves_non_windows_and_refuses_unknown_windows_recording() {
    let mut file = policy_fixture();
    let macos = Target { os: "macos", env: "" };
    assert_eq!(expected(&file, 1, LINUX), (&file["seeds"]["1"], "shared"));
    assert_eq!(expected(&file, 1, macos), (&file["platforms"]["macos"]["seeds"]["1"], "macos"));
    assert_eq!(LINUX.recording_platform(&file), Ok(None));
    assert_eq!(macos.recording_platform(&file), Ok(Some("macos")));
    for env in ["", "other", "new-env"] {
        let unknown = Target { os: "windows", env };
        let before = file.clone();
        assert_eq!(expected(&file, 1, unknown), (&file["seeds"]["1"], "shared"));
        assert!(record(&mut file, 1, &json!({"volume": 30}), "why", unknown).is_err());
        assert_eq!(file, before);
    }
    assert_eq!(expected(&file, 1, Target { os: "future-os", env: "gnu" }).1, "shared");
}

#[test]
fn policy_records_only_changed_gnu_seeds_and_preserves_other_destinations_and_notes() {
    let mut file = policy_fixture();
    let before = file.clone();
    let got = json!({"volume": 30, "outcome": "failed", "error": "known failure"});
    assert_eq!(record(&mut file, 2, &got, "same reason", GNU), Ok(true));
    assert_eq!(file["seeds"], before["seeds"]);
    assert_eq!(file["platforms"]["windows"], before["platforms"]["windows"]);
    assert_eq!(file["platforms"]["macos"], before["platforms"]["macos"]);
    assert_eq!(file["notes"][0], before["notes"][0]);
    assert_eq!(file["notes"][1], json!({"platform": "windows-gnu", "reason": "same reason", "seeds": [2]}));
    assert_eq!(expected(&file, 2, GNU), (&got, "windows-gnu"));
    assert!(file["platforms"]["windows-gnu"]["seeds"].get("1").is_none());
    assert_eq!(record(&mut file, 1, &got, "same reason", GNU), Ok(true));
    assert_eq!(file["notes"][1]["seeds"], json!([1, 2]));
    let unchanged = file.clone();
    assert_eq!(record(&mut file, 1, &got, "same reason", GNU), Ok(false));
    assert_eq!(file, unchanged);
    assert_eq!(record(&mut file, 1, &json!({"volume": 31}), "same reason", GNU), Ok(true));
    assert_eq!(file["notes"][1]["seeds"], json!([1, 2]));
    assert!(expected(&file, 1, GNU).0.get("error").is_none());
    assert_eq!(serde_json::from_str::<Value>(&serialize(&file)).ok().as_ref(), Some(&file));
}

#[test]
fn policy_isolates_msvc_recording_even_without_a_namespace() {
    for target in [MSVC, GNU] {
        let mut file = json!({"seeds": {"1": {"volume": 10}}, "notes": []});
        let before = file.clone();
        assert_eq!(record(&mut file, 1, &json!({"volume": 10}), "why", target), Ok(false));
        assert_eq!(file, before); // No spurious namespace or note for a matching fallback.
        assert_eq!(record(&mut file, 1, &json!({"volume": 11}), "why", target), Ok(true));
        assert_eq!(file["seeds"], before["seeds"]);
        assert_eq!(expected(&file, 1, target).0["volume"], 11);
        assert_eq!(file["notes"][0]["platform"], json!(target.platform()));
    }
}

#[test]
fn policy_notes_never_coalesce_across_destinations_in_either_direction() {
    let mut file = policy_fixture();
    for target in [GNU, MSVC, LINUX, GNU, LINUX, MSVC] {
        let previous_notes = file["notes"].as_array().cloned().unwrap_or_default();
        let got = json!({"volume": 100 + previous_notes.len()});
        assert_eq!(record(&mut file, 1, &got, "same reason", target), Ok(true));
        let notes = file["notes"].as_array().cloned().unwrap_or_default();
        assert_eq!(notes.len(), previous_notes.len() + 1);
        assert_eq!(&notes[..previous_notes.len()], previous_notes);
    }
    let before = file["notes"].clone();
    assert_eq!(record(&mut file, 2, &json!({"volume": 50}), "different reason", MSVC), Ok(true));
    assert_eq!(file["notes"].as_array().map(Vec::len), before.as_array().map(|n| n.len() + 1));
    let before = file.clone();
    assert!(record(&mut file, 1, &json!({"volume": 99}), "  ", GNU).is_err());
    assert_eq!(file, before);
}

#[test]
fn policy_comparator_stays_strict_and_diagnostics_name_the_selected_reference() {
    let want = json!({"volume": 100.0, "area": 10.0, "faces": 2, "outcome": "ok", "steps": ["box"], "error": null, "rejected": 0, "unsupported": 0});
    let mut got = want.clone();
    got["volume"] = json!(100.00005);
    got["area"] = json!(10.000005);
    assert_eq!(differs(&want, &got), None);
    for (field, value) in [
        ("volume", json!(100.001)),
        ("area", json!(10.001)),
        ("faces", json!(3)),
        ("outcome", json!("failed")),
        ("steps", json!(["hole"])),
        ("error", json!("refused")),
        ("rejected", json!(1)),
        ("unsupported", json!(1)),
    ] {
        let mut got = want.clone();
        got[field] = value;
        assert!(differs(&want, &got).is_some(), "{field}");
    }
    let file = policy_fixture();
    let diagnostic = difference(&file, 1, &json!({"volume": 100}), GNU).unwrap_or_default();
    assert!(diagnostic.contains("target_os=windows, target_env=gnu, reference=shared"), "{diagnostic}");
    let diagnostic = difference(&file, 1, &json!({"volume": 100}), MSVC).unwrap_or_default();
    assert!(diagnostic.contains("target_env=msvc, reference=windows"), "{diagnostic}");
}

#[test]
fn policy_non_windows_recording_keeps_existing_os_destination_and_shared_notes() {
    let mut file = policy_fixture();
    let before = file.clone();
    let got = json!({"volume": 40});
    let macos = Target { os: "macos", env: "" };
    assert_eq!(record(&mut file, 1, &got, "same reason", macos), Ok(true));
    assert_eq!(file["seeds"], before["seeds"]);
    assert_eq!(file["platforms"]["windows"], before["platforms"]["windows"]);
    assert_eq!(expected(&file, 1, macos), (&got, "macos"));
    let platforms = file["platforms"].clone();
    assert_eq!(record(&mut file, 1, &got, "same reason", LINUX), Ok(true));
    assert_eq!(file["platforms"], platforms);
    assert_eq!(file["notes"][2], json!({"reason": "same reason", "seeds": [1]}));
    assert_eq!(record(&mut file, 2, &got, "same reason", LINUX), Ok(true));
    assert_eq!(file["notes"][2]["seeds"], json!([1, 2]));
}

/// One test per seed, and the list of them.
macro_rules! seeds {
    ($($name:ident = $seed:literal),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                check($seed);
            }
        )*
        const LISTED: &[u64] = &[$($seed),*];
    };
}

seeds!(
    seed_00 = 0,
    seed_01 = 1,
    seed_02 = 2,
    seed_03 = 3,
    seed_04 = 4,
    seed_05 = 5,
    seed_06 = 6,
    seed_07 = 7,
    seed_08 = 8,
    seed_09 = 9,
    seed_10 = 10,
    seed_11 = 11,
    seed_12 = 12,
    seed_13 = 13,
    seed_14 = 14,
    seed_15 = 15,
    seed_16 = 16,
    seed_17 = 17,
    seed_18 = 18,
    seed_19 = 19,
    seed_20 = 20,
    seed_21 = 21,
    seed_22 = 22,
    seed_23 = 23,
    seed_24 = 24,
    seed_25 = 25,
    seed_26 = 26,
    seed_27 = 27,
    seed_28 = 28,
    seed_29 = 29,
    seed_30 = 30,
    seed_31 = 31,
    seed_32 = 32,
    seed_33 = 33,
    seed_34 = 34,
    seed_35 = 35,
    seed_36 = 36,
    seed_37 = 37,
    seed_38 = 38,
    seed_39 = 39,
    seed_40 = 40,
    seed_41 = 41,
    seed_42 = 42,
    seed_43 = 43,
    seed_44 = 44,
    seed_45 = 45,
    seed_46 = 46,
    seed_47 = 47,
    seed_48 = 48,
    seed_49 = 49,
    seed_50 = 50,
    seed_51 = 51,
    seed_52 = 52,
    seed_53 = 53,
    seed_54 = 54,
    seed_55 = 55,
    seed_56 = 56,
    seed_57 = 57,
    seed_58 = 58,
    seed_59 = 59,
    seed_60 = 60,
    seed_61 = 61,
    seed_62 = 62,
    seed_63 = 63,
    seed_64 = 64,
    seed_65 = 65,
    seed_66 = 66,
    seed_67 = 67,
    seed_68 = 68,
    seed_69 = 69,
    seed_70 = 70,
    seed_71 = 71,
    seed_72 = 72,
    seed_73 = 73,
    seed_74 = 74,
    seed_75 = 75,
    seed_76 = 76,
    seed_77 = 77,
    seed_78 = 78,
    seed_79 = 79,
    seed_80 = 80,
    seed_81 = 81,
    seed_82 = 82,
    seed_83 = 83,
    seed_84 = 84,
    seed_85 = 85,
    seed_86 = 86,
    seed_87 = 87,
    seed_88 = 88,
    seed_89 = 89,
    seed_90 = 90,
    seed_91 = 91,
    seed_92 = 92,
    seed_93 = 93,
    seed_94 = 94,
    seed_95 = 95,
);

/// Every seed in 0..SEEDS has its test, once, and every recorded seed is one of them.
#[test]
fn every_seed_has_a_test() {
    assert_eq!(LISTED, (0..SEEDS).collect::<Vec<_>>().as_slice());
    let file = load();
    let recorded: Vec<u64> = file["seeds"].as_object().into_iter().flatten().filter_map(|(k, _)| k.parse().ok()).collect();
    assert!(recorded.iter().all(|s| *s < SEEDS), "{recorded:?}");
}
