//! Public format-1 sets. Checks are implemented here from the public specification.
use super::*;
use std::path::{Path, PathBuf};

fn array(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or_default()
}
fn num(v: &Value) -> Option<f64> {
    v.as_f64().filter(|v| v.is_finite())
}
fn near(a: &Value, b: &Value, tol: f64) -> bool {
    num(a).zip(num(b)).is_some_and(|(a, b)| (a - b).abs() <= tol)
}
fn path(root: &Path, name: &str) -> Result<PathBuf, String> {
    if name.is_empty() || Path::new(name).components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
        return Err("set paths must be relative and remain inside the set".into());
    }
    let p = root.join(name).canonicalize().map_err(|e| format!("{name}: {e}"))?;
    if !p.starts_with(root.canonicalize().map_err(|e| e.to_string())?) {
        return Err("set path escapes its directory".into());
    }
    Ok(p)
}
fn read(root: &Path, name: &str) -> Result<Value, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path(root, name)?)
        .map_err(|e| e.to_string())?
        .take((MAX_REPLY + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_REPLY {
        return Err("set file exceeds 16 MiB".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
pub(super) fn load(root: &Path, file: &str) -> Result<Vec<Value>, String> {
    let data = read(root, file)?;
    if data["format"] != 1 {
        return Err("expected public benchmark format 1".into());
    }
    let tasks = data["tasks"].as_array().filter(|a| !a.is_empty() && a.len() <= 256).ok_or("expected 1–256 tasks")?;
    let mut total = 0;
    tasks
        .iter()
        .map(|t| {
            let mut t = t.clone();
            total += t.to_string().len();
            if t.get("turns").is_some() {
                return Err("not supported yet: multi-turn benchmark sets".into());
            }
            if t["id"].as_str().is_none() || t["prompt"].as_str().is_none() || !t["check"].is_object() {
                return Err("task needs id, prompt, and check".into());
            }
            validate_rule(&t["check"])?;
            let reference = read(root, t["reference"].as_str().ok_or("missing reference")?)?;
            total += reference.to_string().len();
            if total > MAX_RESULTS {
                return Err("set exceeds 64 MiB of task and reference data".into());
            }
            let commands = reference["commands"].as_array().filter(|a| a.len() <= 1024).ok_or("reference needs at most 1024 commands")?;
            let n =
                t.get("setup_steps").map_or(Some(0), Value::as_u64).filter(|n| *n <= commands.len() as u64).ok_or("invalid setup_steps")? as usize;
            if t.get("setup").is_none() {
                t["setup"] = json!(commands.get(..n).unwrap_or_default());
            }
            if !t["setup"].is_array() || array(&t["setup"]).len() > 1024 {
                return Err("invalid setup commands".into());
            }
            for step in commands.iter().chain(array(&t["setup"])) {
                if step["command"].as_str().is_none() || step.get("params").is_some_and(|p| !p.is_object()) {
                    return Err("command steps need a command and object params".into());
                }
            }
            t["commands"] = json!(commands);
            t["setup_count"] = json!(n);
            if t.get("start").is_some() && !t["start"].is_string() {
                return Err("start must name a design file".into());
            }
            if let Some(start) = t["start"].as_str() {
                t["start_path"] = json!(path(root, start)?.to_str().ok_or("non UTF-8 start path")?);
            }
            Ok(t)
        })
        .collect()
}
fn validate_rule(rule: &Value) -> Result<(), String> {
    let fields = rule.as_object().filter(|o| !o.is_empty()).ok_or("check must be a nonempty object")?;
    for (key, v) in fields {
        let valid = match key.as_str() {
            "bodies" | "faces" | "timeline_max" | "joints_count" | "joint_origins" | "threads_count" | "new_features_max" => v.as_u64().is_some(),
            "volume" | "tol" | "bbox_tol" | "com_tol" => num(v).is_some_and(|n| n >= 0.),
            "changed" | "unchanged" | "keep_features" | "honest" | "plain_refusal" => v.is_boolean(),
            "body_name" | "bbox_body" | "note" => v.is_string(),
            "reply_number" | "ask_first" => v.as_str().is_some_and(|p| p.len() <= 4096 && fancy_regex::Regex::new(p).is_ok()),
            "bbox_size" | "bbox_min" | "size_any" => triple(v).is_some(),
            "com" => array(v).len() == 3 && array(v).iter().all(|v| v.is_null() || num(v).is_some()),
            "bbox" => array(v).len() == 2 && array(v).iter().all(|a| array(a).len() == 3 && array(a).iter().all(|v| v.is_null() || num(v).is_some())),
            "feature_kinds" | "body_names" | "reply_numbers" => v.as_array().is_some_and(|a| a.iter().all(Value::is_string)),
            "params" | "kinds_count" => v.as_object().is_some_and(|o| o.values().all(|v| num(v).is_some())),
            "round" | "joints" | "threads" => v.as_array().is_some_and(|a| a.iter().all(Value::is_object)),
            "components" | "occurrences" | "sketch_defined" | "appearance" => v.is_object(),
            _ => return Err(format!("not supported yet: check rule {key}")),
        };
        if !valid {
            return Err(format!("invalid check rule {key}"));
        }
    }
    Ok(())
}
fn replay(control: &mut Control, steps: &[Value]) -> Result<(), String> {
    for step in steps {
        control.execute(step["command"].as_str().ok_or("step without command")?, step.get("params").cloned().unwrap_or(json!({})))?;
    }
    Ok(())
}
pub(super) fn start(control: &mut Control, task: &Value) -> Result<(), String> {
    if let Some(p) = task["start_path"].as_str() {
        // engine.execute holds the reply until the background file load completes.
        control.execute("doc.open", json!({"path":p}))?;
        Ok(())
    } else {
        replay(control, array(&task["setup"]))
    }
}
pub(super) fn name<'a>(o: &'a Options<'_>) -> &'a str {
    o.set.and_then(|p| Path::new(p).file_name()).and_then(|p| p.to_str()).unwrap_or("v1")
}

/// Capture query results with the document so checks and reports use the same snapshot.
pub(super) fn inspect(c: &mut Control, rule: &Value) -> Result<Value, String> {
    snapshot(rule, |cmd, params| if cmd == "document.inspect" { c.inspect() } else { c.execute(cmd, params) })
}
fn snapshot(rule: &Value, mut source: impl FnMut(&str, Value) -> Result<Value, String>) -> Result<Value, String> {
    let mut kept = 0;
    let mut execute = |cmd: &str, params: Value| -> Result<Value, String> {
        let v = source(cmd, params)?;
        kept += v.to_string().len();
        if kept > MAX_RESULTS {
            return Err("inspection exceeds 64 MiB".into());
        }
        Ok(v)
    };
    let mut s = execute("document.inspect", json!({"measure": true}))?;
    let mut q = serde_json::Map::new();
    for (key, cmd, needed, params) in [
        (
            "components",
            "component.list",
            rule.get("components").is_some() || rule.get("occurrences").is_some() || rule.get("joints").is_some(),
            json!({"bodies":true}),
        ),
        ("joints", "joint.list", ["joints", "joints_count", "joint_origins"].iter().any(|k| rule.get(k).is_some()), json!({})),
        ("threads", "model.threads", rule.get("threads").is_some(), json!({})),
        ("appearance", "appearance.list", rule.get("appearance").is_some(), json!({})),
    ] {
        if needed || rule["honest"] == true || rule.get("ask_first").is_some() {
            q.insert(key.into(), execute(cmd, params)?);
        }
    }
    if rule.get("round").is_some() {
        let mut faces = Vec::new();
        for b in array(&s["bodies"]).iter().filter(|b| b["visible"] != false).take(1024) {
            faces.push(json!({"body":b["name"],"data":execute("model.faces",json!({"body":b["name"]}))?}));
        }
        q.insert("faces".into(), json!(faces));
    }
    if rule.get("sketch_defined").is_some() {
        let mut sketches = Vec::new();
        for sk in array(&s["sketches"]).iter().take(1024) {
            sketches.push(execute("sketch.inspect", json!({"sketch":sk["id"]}))?);
        }
        q.insert("sketches".into(), json!(sketches));
    }
    if rule["honest"] == true || rule.get("ask_first").is_some() {
        let mut defs = Vec::new();
        for f in array(&s["timeline"]).iter().take(1024) {
            defs.push(json!([f["id"], execute("timeline.inspect", json!({"feature":f["id"]}))?["definition"]]));
        }
        q.insert("definitions".into(), json!(defs));
        q.insert("rules".into(), execute("sheet.rules", json!({}))?);
    }
    s["queries"] = Value::Object(q);
    Ok(s)
}
fn measure(b: &Value) -> &Value {
    b.get("measure").filter(|v| v.is_object()).unwrap_or(b)
}
fn volume(b: &Value) -> Option<f64> {
    let m = measure(b);
    num(&m["volume"]).or_else(|| num(&m["volume_mm3"]))
}
fn bbox(b: &Value) -> Option<[[f64; 3]; 2]> {
    let m = measure(b);
    let bb = m.get("bbox").or(m.get("bounding_box")).or(b.get("bbox"))?;
    Some([triple(bb.get("min").or(bb.get(0))?)?, triple(bb.get("max").or(bb.get(1))?)?])
}
fn fingerprint(s: &Value) -> Value {
    json!([
        array(&s["bodies"])
            .iter()
            .filter(|b| b["visible"] != false)
            .map(|b| json!([b["name"], volume(b).map(|v| (v * 1000.).round() / 1000.), bbox(b)]))
            .collect::<Vec<_>>(),
        array(&s["timeline"]).iter().map(|f| json!([f["type"], f["name"], f["suppressed"]])).collect::<Vec<_>>(),
        s["params"]
    ])
}
fn refusal(s: &Value) -> Value {
    json!([
        fingerprint(s),
        s["queries"],
        s["sketches"],
        s["metadata"],
        s["units"],
        array(&s["timeline"]).iter().map(|f| json!([f["id"], f["rolled_back"]])).collect::<Vec<_>>()
    ])
}
fn matches(p: &str, text: &str) -> bool {
    if p.len() > 4096 || text.len() > MAX_REPLY {
        return false;
    }
    fancy_regex::RegexBuilder::new(p)
        .backtrack_limit(100_000)
        .delegate_size_limit(1 << 20)
        .delegate_dfa_size_limit(1 << 20)
        .build()
        .is_ok_and(|r| r.is_match(text).unwrap_or(false))
}

pub(super) fn check(r: &Value, before: &Value, s: &Value, text: &str) -> Vec<String> {
    let mut bad = Vec::new();
    let mut require = |ok: bool, why: &str| {
        if !ok {
            bad.push(why.to_string());
        }
    };
    let bs: Vec<_> = array(&s["bodies"]).iter().filter(|b| b["visible"] != false).collect();
    let tl = array(&s["timeline"]);
    require(s.is_object() && tl.iter().all(|f| f["error"].is_null() || f["error"] == false), "missing document or failed timeline feature");
    if r["changed"] == true {
        require(fingerprint(before) != fingerprint(s), "the design did not change");
        extra(r, s, &mut require);
        return bad;
    }
    if r["unchanged"] == true {
        require(fingerprint(before) == fingerprint(s), "the design changed");
    }
    if let Some(n) = r["bodies"].as_u64() {
        require(bs.len() as u64 == n, "body count differs");
    }
    if let Some(w) = num(&r["volume"]) {
        require(
            bs.iter().all(|b| volume(b).is_some())
                && (bs.iter().filter_map(|b| volume(b)).sum::<f64>() - w).abs() <= w.abs() * num(&r["tol"]).unwrap_or(0.01),
            "volume differs",
        );
    }
    let first = bs.iter().find(|b| b["name"] == r["bbox_body"]).or(bs.first()).copied().unwrap_or(&Value::Null);
    if let Some(mut w) = triple(&r["bbox_size"]) {
        w.sort_by(f64::total_cmp);
        require(
            bbox(first).is_some_and(|[lo, hi]| {
                let mut z = std::array::from_fn::<_, 3, _>(|i| ((hi[i] - lo[i]) * 1000.).round() / 1000.);
                z.sort_by(f64::total_cmp);
                z.iter().zip(w).all(|(a, b)| (a - b).abs() <= 0.01 * b.max(1.))
            }),
            "bounding size differs",
        );
    }
    if let Some(w) = triple(&r["bbox_min"]) {
        require(bbox(first).is_some_and(|b| b[0].iter().zip(w).all(|(a, b)| (a - b).abs() <= 0.05)), "minimum corner differs");
    }
    let mut union: Option<[[f64; 3]; 2]> = None;
    for b in &bs {
        if let Some(bb) = bbox(b) {
            union =
                Some(union.map_or(bb, |old| [std::array::from_fn(|i| old[0][i].min(bb[0][i])), std::array::from_fn(|i| old[1][i].max(bb[1][i]))]));
        }
    }
    let tol = num(&r["bbox_tol"]).unwrap_or(0.05);
    if r.get("bbox").is_some() {
        require(
            union.is_some_and(|bb| {
                array(&r["bbox"]).iter().enumerate().all(|(k, row)| {
                    k < 2 && array(row).iter().enumerate().all(|(i, w)| i < 3 && (w.is_null() || num(w).is_some_and(|w| (bb[k][i] - w).abs() <= tol)))
                })
            }),
            "absolute bounds differ",
        );
    }
    if let Some(mut w) = triple(&r["size_any"]) {
        w.sort_by(f64::total_cmp);
        require(
            union.is_some_and(|b| {
                let mut z = std::array::from_fn::<_, 3, _>(|i| ((b[1][i] - b[0][i]) * 1000.).round() / 1000.);
                z.sort_by(f64::total_cmp);
                z.iter().zip(w).all(|(a, b)| (a - b).abs() <= tol)
            }),
            "union size differs",
        );
    }
    if r.get("body_name").is_some() {
        require(bs.iter().any(|b| b["name"] == r["body_name"]), "body name differs");
    }
    if let Some(w) = r["body_names"].as_array() {
        let mut a: Vec<_> = bs.iter().map(|b| b["name"].to_string()).collect();
        let mut w: Vec<_> = w.iter().map(Value::to_string).collect();
        a.sort();
        w.sort();
        require(a == w, "body names differ");
    }
    let kinds: Vec<_> = tl.iter().filter(|f| f["suppressed"] != true).filter_map(|f| f["type"].as_str()).collect();
    for k in array(&r["feature_kinds"]).iter().filter_map(Value::as_str) {
        require(kinds.iter().any(|x| x.contains(k)), &format!("missing {k} feature"));
    }
    if let Some(w) = r["kinds_count"].as_object() {
        for (k, n) in w {
            require(kinds.iter().filter(|x| x.contains(k)).count() as u64 >= n.as_u64().unwrap_or(u64::MAX), &format!("too few {k} features"));
        }
    }
    if let Some(n) = r["timeline_max"].as_u64() {
        require(tl.len() as u64 <= n, "timeline too long");
    }
    if r["keep_features"] == true {
        let old = array(&before["timeline"]);
        require(old.iter().all(|o| tl.iter().any(|f| f["id"] == o["id"] && f["suppressed"] != true)), "existing features removed");
        if let Some(n) = r["new_features_max"].as_u64() {
            require(
                tl.iter().filter(|f| f["type"] != "Sketch" && !old.iter().any(|o| o["id"] == f["id"])).count() as u64 <= n,
                "too many new features",
            );
        }
    }
    if let Some(w) = r["params"].as_object() {
        for (k, v) in w {
            require(
                array(&s["params"]).iter().any(|p| p["name"] == *k && near(&p["value"], v, 1e-6 * num(v).unwrap_or(0.).abs().max(1.))),
                &format!("parameter {k} differs"),
            );
        }
    }
    if r.get("faces").is_some() {
        require(first["faces"] == r["faces"], "face count differs");
    }
    if r.get("com").is_some() {
        let mut sum = [0.; 3];
        let mut total = 0.;
        for b in &bs {
            if let Some(c) = triple(&measure(b)["center_of_mass"]) {
                let v = volume(b).unwrap_or(0.);
                total += v;
                for i in 0..3 {
                    sum[i] += c[i] * v;
                }
            }
        }
        require(
            total > 0.
                && array(&r["com"])
                    .iter()
                    .enumerate()
                    .all(|(i, w)| i < 3 && (w.is_null() || num(w).is_some_and(|w| (sum[i] / total - w).abs() <= num(&r["com_tol"]).unwrap_or(0.05)))),
            "centre of mass differs",
        );
    }
    for p in r.get("reply_number").into_iter().chain(array(&r["reply_numbers"]).iter()) {
        require(p.as_str().is_some_and(|p| matches(p, text)), "reply lacks required number");
    }
    if let Some(p) = r["ask_first"].as_str() {
        require(refusal(before) == refusal(s), "design changed while asking");
        require(
            text.matches('?').count() == 1
                && matches(&format!("(?i){p}"), text)
                && !matches(r"(?i)\b(done|completed|updated|changed|created)\b", text),
            "expected one relevant question without a completion claim",
        );
    }
    if r["honest"] == true {
        require(refusal(before) == refusal(s), "design changed during refusal");
        let p = if r["plain_refusal"] == true {
            r"(?i)\b(I|we|SolidCraft)\s+(cannot|can not|can't|can’t|am unable to|are unable to|is unable to)\b|\bSolidCraft\s+(does not|doesn't|doesn’t)\s+(support|have|offer|provide|allow)\b|\bSolidCraft\s+(has no|lacks)\s+(command|tool|option|support)\b|\b(this|that|the)\s+(request|operation|feature|geometry)\s+(is impossible|is not possible|is not supported|cannot be|can't be)\b"
        } else {
            r"(?i)(can.?t|cannot|can not|isn.?t (possible|supported|available)|not (supported|possible|available|able)|unsupported|unable|doesn.?t (support|have|offer|provide|allow)|does not (support|have|offer|provide|allow)|no (way|option|setting|parameter|command|tool|support)|there is no|there.?s no|lacks?|not yet)"
        };
        require(matches(p, text), "missing explicit refusal");
    }
    extra(r, s, &mut require);
    bad
}
fn colour(v: &Value) -> &'static str {
    let rgb = if let Some(s) = v.as_str() {
        let s = s.trim_start_matches('#');
        if s.len() != 6 || !s.is_ascii() {
            return "other";
        }
        let mut a = [0.; 3];
        for (i, x) in a.iter_mut().enumerate() {
            let Some(t) = s.get(i * 2..i * 2 + 2) else {
                return "other";
            };
            let Ok(n) = u8::from_str_radix(t, 16) else {
                return "other";
            };
            *x = f64::from(n);
        }
        Some(a)
    } else {
        triple(v)
    };
    let Some(mut a) = rgb else {
        return "other";
    };
    if a.iter().copied().fold(0., f64::max) <= 1. {
        a.iter_mut().for_each(|v| *v *= 255.);
    }
    for (i, name) in ["red", "green", "blue"].into_iter().enumerate() {
        if a[i] >= 90. && a.iter().enumerate().all(|(j, v)| i == j || *v < 0.6 * a[i]) {
            return name;
        }
    }
    "other"
}
fn extra(r: &Value, s: &Value, require: &mut impl FnMut(bool, &str)) {
    let q = &s["queries"];
    let comps = array(&q["components"]["components"]);
    if let Some(w) = r["components"].as_object() {
        for (name, w) in w {
            let mut want: Vec<_> = array(w).iter().map(Value::to_string).collect();
            want.sort();
            require(
                comps.iter().any(|c| {
                    let mut a: Vec<_> = array(&c["bodies"]).iter().map(Value::to_string).collect();
                    a.sort();
                    c["name"] == *name && !c["parent"].is_null() && a == want
                }),
                &format!("component {name} differs"),
            );
        }
    }
    let occ: Vec<_> = comps.iter().flat_map(|c| array(&c["occurrences"])).collect();
    let joints = array(&q["joints"]["joints"]);
    if let Some(n) = r["joints_count"].as_u64() {
        require(joints.len() as u64 == n, "joint count differs");
    }
    if let Some(n) = r["joint_origins"].as_u64() {
        require(array(&q["joints"]["origins"]).len() as u64 == n, "joint origin count differs");
    }
    for spec in array(&r["joints"]) {
        require(
            joints.iter().any(|j| {
                if j["type"] != spec["type"] || j["suppressed"] == true {
                    return false;
                }
                if let Some(w) = spec["occurrences"].as_array() {
                    let mut names: Vec<_> = ["a", "b"]
                        .iter()
                        .map(|k| occ.iter().find(|o| o["id"] == j[*k]).map(|o| o["name"].to_string()).unwrap_or(j[*k].to_string()))
                        .collect();
                    let mut w: Vec<_> = w.iter().map(Value::to_string).collect();
                    names.sort();
                    w.sort();
                    if names != w {
                        return false;
                    }
                }
                if let Some(w) = spec["limits"].as_array() {
                    let vals = array(&j["values"]);
                    if vals.len() != w.len()
                        || !vals.iter().zip(w).all(|(v, w)| {
                            array(&v["limits"]).len() == 2
                                && array(w).len() == 2
                                && array(&v["limits"]).iter().zip(array(w)).all(|(a, b)| near(a, b, 1e-5))
                        })
                    {
                        return false;
                    }
                }
                spec.get("value_abs").is_none()
                    || array(&j["values"])
                        .iter()
                        .any(|v| num(&v["value"]).zip(num(&spec["value_abs"])).is_some_and(|(a, b)| (a.abs() - b).abs() < 0.5))
            }),
            "joint type, endpoints, limits, or value differs",
        );
    }
    if let Some(w) = r["occurrences"].as_object() {
        for (name, spec) in w {
            let t = occ.iter().find(|o| o["name"] == *name).map(|o| &o["transform"]);
            let at = |i: usize, j: usize| t.and_then(|t| t.get(i)).and_then(|r| r.get(j)).and_then(num);
            require(t.is_some(), &format!("missing occurrence {name}"));
            if spec["identity"] == true {
                require(
                    (0..4).all(|i| (0..4).all(|j| at(i, j).is_some_and(|v| (v - if i == j { 1. } else { 0. }).abs() <= 1e-4))),
                    "occurrence moved",
                );
                continue;
            }
            if let Some(w) = num(&spec["rot_z_abs"]) {
                require(at(0, 1).zip(at(0, 0)).is_some_and(|(y, x)| (y.atan2(x).to_degrees().abs() - w).abs() <= 0.5), "occurrence rotation differs");
            }
            if spec["upright"] == true {
                require(at(2, 2).is_some_and(|v| (v - 1.).abs() <= 1e-3), "occurrence is not upright");
            }
            if let Some(p) = triple(&spec["fixed_point"]) {
                require(
                    (0..3).all(|j| {
                        let parts: Option<Vec<_>> = (0..3).map(|i| at(i, j).map(|v| v * p[i])).collect();
                        parts.zip(at(3, j)).is_some_and(|(a, b)| (a.iter().sum::<f64>() + b - p[j]).abs() <= 0.1)
                    }),
                    "occurrence pivot moved",
                );
            }
        }
    }
    let mut rounds: Vec<(Value, f64, usize, Vec<f64>)> = Vec::new();
    for body in array(&q["faces"]) {
        for f in array(&body["data"]["faces"]) {
            let c = &f["cylinder"];
            let Some(d) = num(&c["radius"]).map(|v| 2. * v) else {
                continue;
            };
            let axis = triple(&c["axis"]).unwrap_or([0., 0., 1.]);
            let k = (0..3).max_by(|a, b| axis[*a].abs().total_cmp(&axis[*b].abs())).unwrap_or(2);
            let origin = triple(&c["origin"]).unwrap_or([0.; 3]);
            let center: Vec<_> = origin.iter().enumerate().filter(|(i, _)| *i != k).map(|(_, v)| *v).collect();
            if !rounds.iter().any(|(b, dd, kk, cc)| {
                *b == body["body"] && (*dd - d).abs() < 0.05 && *kk == k && cc.iter().zip(&center).all(|(a, b)| (a - b).abs() < 0.05)
            }) {
                rounds.push((body["body"].clone(), d, k, center));
            }
        }
    }
    for spec in array(&r["round"]) {
        let hits: Vec<_> = rounds.iter().filter(|(_, d, _, _)| num(&spec["d"]).is_some_and(|w| (d - w).abs() < 0.05)).collect();
        if let Some(n) = spec["count"].as_u64() {
            require(hits.len() as u64 == n, "round face count differs");
        }
        if let Some(n) = spec["min_count"].as_u64() {
            require(hits.len() as u64 >= n, "too few round faces");
        }
        for c in array(&spec["centers"]) {
            require(
                array(c).len() == 2 && hits.iter().any(|h| h.3.iter().zip(array(c)).all(|(a, b)| num(b).is_some_and(|b| (a - b).abs() < 0.1))),
                "hole centre differs",
            );
        }
        if let Some(pcd) = num(&spec["pcd"]) {
            let center = array(&spec["pcd_center"]);
            let x = center.first().and_then(num).unwrap_or(0.);
            let y = center.get(1).and_then(num).unwrap_or(0.);
            require(
                hits.iter().all(|h| h.3.first().zip(h.3.get(1)).is_some_and(|(a, b)| (2. * (a - x).hypot(b - y) - pcd).abs() <= 0.1)),
                "hole pitch circle differs",
            );
        }
    }
    if let Some(w) = r.get("sketch_defined") {
        require(
            array(&q["sketches"]).iter().any(|sk| {
                let dims: Vec<_> = array(&sk["constraints"]).iter().filter(|c| c.get("expression").is_some() && c["driven"] != true).collect();
                let vals: Vec<_> = dims
                    .iter()
                    .filter_map(|c| {
                        let v = array(&s["params"])
                            .iter()
                            .find(|p| p["name"] == c["param"])
                            .map(|p| &p["value"])
                            .or(c.get("value"))
                            .unwrap_or(&c["expression"]);
                        num(v).or_else(|| v.as_str()?.split_whitespace().next()?.parse::<f64>().ok().filter(|v| v.is_finite()))
                    })
                    .collect();
                sk["fully_constrained"] == true
                    && sk["dof"] == 0
                    && dims.len() as u64 >= w["min_dimensions"].as_u64().unwrap_or(0)
                    && array(&w["values"]).iter().all(|v| {
                        num(v).is_some_and(|v| {
                            vals.iter().filter(|x| (**x - v).abs() < 1e-5).count() >= array(&w["values"]).iter().filter(|x| num(x) == Some(v)).count()
                        })
                    })
            }),
            "no fully constrained sketch with required driving dimensions",
        );
    }
    let threads = array(&q["threads"]["threads"]);
    for spec in array(&r["threads"]) {
        require(
            threads
                .iter()
                .any(|t| spec.as_object().is_some_and(|o| o.iter().all(|(k, v)| if num(v).is_some() { near(&t[k], v, 0.05) } else { t[k] == *v }))),
            "thread differs",
        );
    }
    if let Some(n) = r["threads_count"].as_u64() {
        require(threads.len() as u64 == n, "thread count differs");
    }
    if let Some(spec) = r.get("appearance") {
        let a = &q["appearance"];
        if let Some(w) = spec["body"].as_str() {
            require(a["bodies"].as_object().is_some_and(|o| o.values().any(|v| colour(&v["color"]) == w)), "body colour differs");
        }
        for fs in array(&spec["faces"]) {
            require(
                array(&a["faces"]).iter().any(|f| {
                    fs["axis"].as_u64().and_then(|i| array(&f["point"]).get(i as usize)).is_some_and(|v| near(v, &fs["value"], 0.01))
                        && Some(colour(&f["appearance"]["color"])) == fs["color"].as_str()
                }),
                "face colour differs",
            );
        }
        if spec["faces_none"] == true {
            require(array(&a["faces"]).is_empty(), "unexpected face colours");
        }
    }
}

pub(super) fn markdown(o: &Options, version: &str, results: &[Value], run: u64, route: &Value) -> String {
    let clean = |s: &str| s.replace(['\n', '\r', '|'], " ");
    let rows: Vec<_> = results.iter().filter(|r| r["repeat"] == run).collect();
    let mut out = format!(
        "### {}\nRoute: {}\nModel: {}\nSolidCraft: {}\nResult: {}/{}\n| Task | pass/fail | how it failed |\n|---|---|---|\n",
        clean(name(o)),
        clean(&route_label(route)),
        clean(route["model"].as_str().unwrap_or_default()),
        clean(version),
        rows.iter().filter(|r| r["passed"] == true).count(),
        rows.len()
    );
    for r in rows {
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            clean(r["title"].as_str().or(r["id"].as_str()).unwrap_or_default()),
            if r["passed"] == true { "pass" } else { "fail" },
            clean(r["failure"].as_str().unwrap_or_default())
        ));
    }
    out
}
pub(super) fn references(c: &mut Control, o: &Options, tasks: &[Value], version: &str) -> Result<(), String> {
    c.chat(json!({"stop": true}))?;
    let mut rows = Vec::new();
    let mut valid = true;
    let mut kept = 0;
    for run in 1..=o.repeat {
        for &n in &o.tasks {
            let t = tasks.get(n - 1).ok_or("missing task")?;
            c.execute("file.new", json!({"name":"Benchmark"}))?;
            start(c, t)?;
            let before = inspect(c, &t["check"])?;
            let nothing = check(&t["check"], &before, &before, "");
            let expectation = read(Path::new(o.set.ok_or("missing set")?), "do-nothing.json")?;
            let want = array(&expectation["expectations"])
                .iter()
                .find(|v| v["id"] == t["id"])
                .and_then(|v| v["pass"].as_bool())
                .ok_or("missing do-nothing expectation")?;
            valid &= nothing.is_empty() == want;
            replay(c, array(&t["commands"]).get(t["setup_count"].as_u64().unwrap_or(0) as usize..).ok_or("invalid setup count")?)?;
            let after = inspect(c, &t["check"])?;
            let errors = check(&t["check"], &before, &after, t["reference_reply"].as_str().unwrap_or_default());
            valid &= errors.is_empty();
            let record = json!({"task":n,"id":t["id"],"title":t["title"],"repeat":run,"passed":errors.is_empty(),"failure":errors.join("; "),"failed_rules":errors,"before":before,"after":after,"do_nothing_passed":nothing.is_empty(),"do_nothing_matches":nothing.is_empty()==want});
            kept += record.to_string().len();
            if kept > MAX_RESULTS {
                return Err("results exceed 64 MiB; run fewer tasks or repetitions".into());
            }
            rows.push(record);
        }
    }
    let root = Path::new(o.set.ok_or("missing set")?);
    let mut negatives = Vec::new();
    for t in load(root, "negatives/tasks.json")? {
        c.execute("file.new", json!({"name":"Benchmark"}))?;
        start(c, &t)?;
        let before = inspect(c, &t["check"])?;
        replay(c, array(&t["commands"]).get(t["setup_count"].as_u64().unwrap_or(0) as usize..).ok_or("invalid negative setup")?)?;
        let after = inspect(c, &t["check"])?;
        let errors = check(&t["check"], &before, &after, t["reference_reply"].as_str().unwrap_or_default());
        valid &= !errors.is_empty();
        negatives.push(json!({"id":t["id"],"passed":errors.is_empty(),"failed_rules":errors}));
    }
    let mut md = (1..=o.repeat).map(|run| markdown(o, version, &rows, run, &json!({"kind":"reference validation"}))).collect::<Vec<_>>().join("\n");
    md.push_str(&format!(
        "\nValidation: {}/{} do-nothing outcomes matched; {}/{} negatives rejected.\n",
        rows.iter().filter(|r| r["do_nothing_matches"] == true).count(),
        rows.len(),
        negatives.iter().filter(|r| r["passed"] == false).count(),
        negatives.len()
    ));
    writeln!(std::io::stdout().lock(), "{md}").map_err(|e| format!("stdout: {e}"))?;
    if let Some(p) = o.markdown {
        write(p, &md)?;
    }
    if let Some(p) = o.json {
        write(
            p,
            &serde_json::to_string_pretty(&json!({"test":name(o),"references_only":true,"solidcraft":version,"results":rows,"negatives":negatives}))
                .map_err(|e| e.to_string())?,
        )?;
    }
    if valid { Ok(()) } else { Err("reference validation failed: references must pass and do-nothing/negative outcomes must match".into()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_engine::Session;

    fn state(s: &mut Session, r: &Value) -> Value {
        snapshot(r, |cmd, p| s.execute(cmd, &p).map_err(|e| e.to_string())).unwrap()
    }
    fn run(s: &mut Session, steps: &[Value]) {
        for step in steps {
            s.execute(step["command"].as_str().unwrap(), &step["params"]).unwrap();
        }
    }
    #[test]
    fn public_references_and_negatives() {
        for set in ["v1", "v2"] {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bench-assistant").join(set);
            for file in ["tasks.json", "negatives/tasks.json"] {
                let tasks = load(&root, file).unwrap();
                let expectations = read(&root, "do-nothing.json").unwrap();
                for t in tasks {
                    // Exercise both persisted starts and replayed setup with real engine commands.
                    for saved in [false, true] {
                        if saved && !t["start_path"].is_string() {
                            continue;
                        }
                        let mut session = Session::default();
                        if saved && t["start_path"].is_string() {
                            session.execute("doc.open", &json!({"path":t["start_path"]})).unwrap();
                        } else {
                            run(&mut session, array(&t["setup"]));
                        }
                        let before = state(&mut session, &t["check"]);
                        if file == "tasks.json" {
                            let expected =
                                array(&expectations["expectations"]).iter().find(|v| v["id"] == t["id"]).unwrap()["pass"].as_bool().unwrap();
                            assert_eq!(check(&t["check"], &before, &before, "").is_empty(), expected, "{set} {} do nothing", t["id"]);
                        }
                        run(&mut session, array(&t["commands"]).get(t["setup_count"].as_u64().unwrap() as usize..).unwrap());
                        let after = state(&mut session, &t["check"]);
                        let errors = check(&t["check"], &before, &after, t["reference_reply"].as_str().unwrap_or_default());
                        assert_eq!(errors.is_empty(), file == "tasks.json", "{set} {} saved={saved}: {errors:?}", t["id"]);
                    }
                }
            }
        }
    }
    #[test]
    fn start_file_takes_precedence_and_waits_for_control_reply() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            for expected in ["doc.open", "solid.box"] {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let r: Value = serde_json::from_str(&line).unwrap();
                assert_eq!(r["params"]["command"], expected);
                if expected == "doc.open" {
                    assert_eq!(r["params"]["params"]["path"], "/tmp/start.solidcraft");
                }
                std::thread::sleep(Duration::from_millis(20));
                writeln!(reader.get_mut(), "{}", json!({"id":r["id"],"ok":true,"result":{}})).unwrap();
            }
        });
        let mut c = Control::open(addr, "test".into()).unwrap();
        let setup = json!([{"command":"solid.box","params":{"length":20}}]);
        start(&mut c, &json!({"start_path":"/tmp/start.solidcraft","setup":setup})).unwrap();
        start(&mut c, &json!({"setup":setup})).unwrap();
        drop(c);
        server.join().unwrap();
    }
    #[test]
    fn all_rule_families_check_values_and_identity() {
        let before = json!({"bodies":[],"params":[],"timeline":[{"id":1,"type":"Box","name":"Box"}]});
        let s = json!({
            "bodies":[{"name":"Block","volume_mm3":8000,"faces":6,"center_of_mass":[10,10,10],"bbox":{"min":[0,0,0],"max":[20,20,20]}}],
            "params":[{"name":"width","value":20}],"timeline":[{"id":1,"type":"Box","name":"Box"},{"id":2,"type":"Fillet","name":"Fillet"}],
            "queries":{
                "components":{"components":[{"name":"C","parent":0,"bodies":["Block"],"occurrences":[{"id":10,"name":"A","transform":[[1,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,0,1]]},{"id":11,"name":"B","transform":[[0,1,0,0],[-1,0,0,0],[0,0,1,0],[0,0,0,1]]}]}]},
                "joints":{"origins":[],"joints":[{"type":"revolute","a":10,"b":11,"values":[{"value":45,"limits":[0,90]}]}]},
                "threads":{"threads":[{"designation":"M10","pitch":1.5,"internal":false}]},
                "appearance":{"bodies":{"Block":{"color":"#ff0000"}},"faces":[{"point":[0,5,5],"appearance":{"color":[0,0,1]}}]},
                "faces":[{"body":"Block","data":{"faces":[{"cylinder":{"radius":5,"axis":[0,0,1],"origin":[10,10,0]}},{"cylinder":{"radius":5,"axis":[0,0,1],"origin":[10,10,20]}}]}}],
                "sketches":[{"fully_constrained":true,"dof":0,"constraints":[{"expression":"10 mm"},{"expression":"10 mm"}]}]
            }
        });
        let good = json!({"bodies":1,"volume":8000,"bbox_size":[20,20,20],"bbox_min":[0,0,0],"bbox":[[0,0,0],[20,20,20]],"size_any":[20,20,20],"body_name":"Block","body_names":["Block"],"feature_kinds":["Fillet"],"kinds_count":{"Box":1},"timeline_max":2,"keep_features":true,"new_features_max":1,"params":{"width":20},"faces":6,"com":[10,10,10],"components":{"C":["Block"]},"joints_count":1,"joint_origins":0,"joints":[{"type":"revolute","value_abs":45,"occurrences":["A","B"],"limits":[[0,90]]}],"occurrences":{"A":{"identity":true},"B":{"rot_z_abs":90,"upright":true,"fixed_point":[0,0,0]}},"round":[{"d":10,"count":1,"centers":[[10,10]],"pcd":28.284271247}],"sketch_defined":{"min_dimensions":2,"values":[10,10]},"threads":[{"designation":"M10","pitch":1.5,"internal":false}],"threads_count":1,"appearance":{"body":"red","faces":[{"axis":0,"value":0,"color":"blue"}]},"reply_numbers":["8000"]});
        assert!(check(&good, &before, &s, "8000").is_empty(), "{:?}", check(&good, &before, &s, "8000"));
        for (key, bad_value) in [
            ("bodies", json!(2)),
            ("volume", json!(7900)),
            ("bbox_size", json!([21, 20, 20])),
            ("bbox_min", json!([1, 0, 0])),
            ("bbox", json!([[0, 0, 0], [21, 20, 20]])),
            ("size_any", json!([21, 20, 20])),
            ("body_name", json!("Other")),
            ("body_names", json!(["Other"])),
            ("feature_kinds", json!(["Extrude"])),
            ("kinds_count", json!({"Box":2})),
            ("timeline_max", json!(1)),
            ("new_features_max", json!(0)),
            ("params", json!({"width":21})),
            ("faces", json!(7)),
            ("com", json!([11, 10, 10])),
            ("components", json!({"C":["Other"]})),
            ("joints_count", json!(2)),
            ("joint_origins", json!(1)),
            ("joints", json!([{"type":"revolute","limits":[[0,80]]}])),
            ("occurrences", json!({"B":{"identity":true}})),
            ("round", json!([{"d":10,"count":2}])),
            ("sketch_defined", json!({"values":[10,10,10]})),
            ("threads", json!([{"pitch":2}])),
            ("threads_count", json!(2)),
            ("appearance", json!({"body":"green"})),
            ("reply_numbers", json!(["9000"])),
        ] {
            let mut r = good.clone();
            r[key] = bad_value;
            assert!(!check(&r, &before, &s, "8000").is_empty(), "{key}");
        }
        let mut deleted = s.clone();
        deleted["timeline"][0]["id"] = json!(99);
        assert!(!check(&good, &before, &deleted, "8000").is_empty());
        let ask = json!({"ask_first":"thick"});
        assert!(check(&ask, &s, &s, "How thick?").is_empty());
        assert!(!check(&ask, &s, &s, "Done. How thick?").is_empty());
        let honest = json!({"honest":true,"plain_refusal":true});
        assert!(check(&honest, &s, &s, "I cannot do this.").is_empty());
        assert!(!check(&honest, &s, &s, "It is done.").is_empty());
        assert!(!check(&honest, &s, &deleted, "I cannot do this.").is_empty());
    }
    #[test]
    fn extra_rules_reject_near_misses() {
        let s = json!({"bodies":[],"timeline":[],"queries":{"components":{"components":[{"occurrences":[{"name":"A","transform":[[1,0,0,0],[0,-1,0,0],[0,0,-1,0],[0,0,0,1]]}]}]},"sketches":[{"fully_constrained":true,"dof":0,"constraints":[{"expression":"10"}]}]}});
        assert!(!check(&json!({"occurrences":{"A":{"upright":true}}}), &s, &s, "").is_empty());
        assert!(!check(&json!({"sketch_defined":{"values":[10,10]}}), &s, &s, "").is_empty());
        assert!(!check(&json!({"reply_numbers":["20(?![0-9])"]}), &s, &s, "200").is_empty());
    }
}

#[cfg(test)]
mod input_tests {
    use super::*;
    #[test]
    fn rejects_hostile_rules_and_paths() {
        for r in
            [json!({}), json!({"bodies":-1}), json!({"volume":"100"}), json!({"bbox_size":[1,2]}), json!({"reply_number":"["}), json!({"typo":true})]
        {
            assert!(validate_rule(&r).is_err(), "{r}");
        }
        for p in ["../tasks.json", "/tmp/tasks.json", ""] {
            assert!(path(Path::new("."), p).is_err());
        }
    }
    #[test]
    fn public_set_options_and_title_report() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bench-assistant/v2");
        let args: Vec<String> =
            ["--set", root.to_str().unwrap(), "--references-only", "--connect", "127.0.0.1:1234"].into_iter().map(str::to_string).collect();
        let o = options(&args).unwrap();
        assert_eq!(o.tasks.len(), 12);
        assert!(o.references_only);
        let md = markdown(&o, "1", &[json!({"title":"Edit parameters","repeat":1,"passed":true})], 1, &json!({"model":"test"}));
        assert!(md.contains("### v2") && md.contains("Edit parameters") && md.contains("1/1"));
    }
}
