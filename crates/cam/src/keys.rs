//! Files a newer build may have written: a machine profile, a machine model, a workholding.
//! Their unknown keys are left out and named, so a profile with a key from a newer build still
//! works here, and the reply says what this build did not read. A job's own keys stay strict:
//! a misspelt key in a job is refused, never read as nothing.

use serde::de::DeserializeOwned;
use serde_json::Value;
use serde_path_to_error::Segment;

/// The most unknown keys left out of one file.
pub const MAX_IGNORED: usize = 200;

/// `text` read as a `T`, its unknown keys (at any depth) left out: the value and the keys left
/// out, each with its path (`rotary.cover`, `parts[3].weight`). Anything else wrong (a value of
/// the wrong type, an unknown variant, a missing key) is an error naming its path.
pub fn from_json_lenient<T: DeserializeOwned>(text: &str) -> Result<(T, Vec<String>), String> {
    let mut v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    from_value_lenient(&mut v)
}

/// [`from_json_lenient`] for a value already read; the unknown keys are removed from `v`.
pub fn from_value_lenient<T: DeserializeOwned>(v: &mut Value) -> Result<(T, Vec<String>), String> {
    let mut ignored: Vec<String> = Vec::new();
    loop {
        let err = match serde_path_to_error::deserialize::<_, T>(&*v) {
            Ok(t) => return Ok((t, ignored)),
            Err(e) => e,
        };
        let path: Vec<Segment> = err.path().iter().cloned().collect();
        let shown = err.path().to_string();
        let message = err.into_inner().to_string();
        let Some(key) = unknown_field(&message) else {
            return Err(if shown == "." { message } else { format!("`{shown}`: {message}") });
        };
        if ignored.len() >= MAX_IGNORED {
            return Err(format!("more than {MAX_IGNORED} unknown keys"));
        }
        // The path names the object the key is in (or the key itself, last).
        let mut at = path.as_slice();
        if let Some((Segment::Map { key: last }, rest)) = at.split_last()
            && *last == key
            && !holds(v, at, &key)
        {
            at = rest;
        }
        if !remove(v, at, &key) {
            return Err(if shown == "." { message } else { format!("`{shown}`: {message}") });
        }
        ignored.push(join(at, &key));
    }
}

/// The key an "unknown field" error names.
fn unknown_field(message: &str) -> Option<String> {
    let rest = message.strip_prefix("unknown field `")?;
    let end = rest.find('`')?;
    Some(rest[..end].to_string())
}

/// The value at `path` in `v`.
fn at_path<'a>(v: &'a mut Value, path: &[Segment]) -> Option<&'a mut Value> {
    let mut cur = v;
    for s in path {
        cur = match s {
            Segment::Map { key } => cur.as_object_mut()?.get_mut(key)?,
            Segment::Seq { index } => cur.as_array_mut()?.get_mut(*index)?,
            // An externally tagged enum: `{variant: {...}}`.
            Segment::Enum { variant } => {
                if cur.as_object().is_some_and(|o| o.contains_key(variant)) {
                    cur.as_object_mut()?.get_mut(variant)?
                } else {
                    cur
                }
            }
            Segment::Unknown => return None,
        };
    }
    Some(cur)
}

fn holds(v: &mut Value, path: &[Segment], key: &str) -> bool {
    at_path(v, path).and_then(|o| o.as_object_mut()).is_some_and(|o| o.contains_key(key))
}

/// Removes `key` from the object at `path`; false when there is no such key there.
fn remove(v: &mut Value, path: &[Segment], key: &str) -> bool {
    at_path(v, path).and_then(|o| o.as_object_mut()).is_some_and(|o| o.remove(key).is_some())
}

fn join(path: &[Segment], key: &str) -> String {
    let mut out = String::new();
    for s in path {
        match s {
            Segment::Map { key } => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(key);
            }
            Segment::Seq { index } => out.push_str(&format!("[{index}]")),
            Segment::Enum { variant } => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(variant);
            }
            Segment::Unknown => out.push_str(".?"),
        }
    }
    if !out.is_empty() {
        out.push('.');
    }
    out.push_str(key);
    out
}

/// The sentence a reply gives for keys left out of `what` (a file and its path).
pub fn ignored_warning(what: &str, keys: &[String]) -> Option<String> {
    if keys.is_empty() {
        return None;
    }
    let list: Vec<String> = keys.iter().map(|k| format!("`{k}`")).collect();
    Some(format!(
        "{what}: this build does not know {} {} and left {} out (written by a newer build, or misspelt)",
        if keys.len() == 1 { "the key" } else { "the keys" },
        list.join(", "),
        if keys.len() == 1 { "it" } else { "them" }
    ))
}
