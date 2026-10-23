//! Skills installed in the agent's working folder, for agents that cannot read files themselves.
//!
//! The built-in chat installs the bundled `solidcraft-*` skills into
//! `<workdir>/.claude/skills/`, and ACP agents (Claude Code, Codex, …) discover them through their
//! own skill system. An API agent (DeepSeek, Ollama, OpenAI, …) runs its tool loop here instead,
//! so it gets the same files through two tools: `list_skills` (the index) and `read_skill` (one
//! file). Only the index goes into the system prompt, so a skill's text is read when the task
//! calls for it, and its `reference.md`, scripts and templates stay reachable.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// Longest file `read_skill` returns; a larger file is cut and says so.
const MAX_TEXT: usize = 400_000;

/// A skill found in the skills folder.
pub struct Skill {
    pub name: String,
    pub description: String,
    pub dir: PathBuf,
}

/// The frontmatter field `key` of a `SKILL.md` (`---` fenced, one line each).
fn frontmatter(text: &str, key: &str) -> Option<String> {
    let body = text.strip_prefix("---")?;
    let end = body.find("\n---")?;
    for line in body.get(..end)?.lines() {
        if let Some(v) = line.trim().strip_prefix(&format!("{key}:")) {
            let v = v.trim().trim_matches('"').trim_matches('\'').trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Every skill in `dir` that has a `SKILL.md`, sorted by folder name.
pub fn scan(dir: &Path) -> Vec<Skill> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Skill> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let path = e.path();
            let text = std::fs::read_to_string(path.join("SKILL.md")).ok()?;
            let name = frontmatter(&text, "name").unwrap_or_else(|| e.file_name().to_string_lossy().into_owned());
            let description = frontmatter(&text, "description").unwrap_or_default();
            Some(Skill { name, description, dir: path })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The files of a skill, relative to its folder, `/`-separated, sorted; at most `limit`.
fn files(dir: &Path, prefix: &str, limit: usize, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut items: Vec<_> = entries.flatten().collect();
    items.sort_by_key(|e| e.file_name());
    for e in items {
        if out.len() >= limit {
            return;
        }
        let name = format!("{prefix}{}", e.file_name().to_string_lossy());
        if e.path().is_dir() {
            files(&e.path(), &format!("{name}/"), limit, out);
        } else {
            out.push(name);
        }
    }
}

/// `list_skills`: the skills, what each is for, and the files it holds.
pub fn list(dir: &Path) -> Value {
    let skills: Vec<Value> = scan(dir)
        .iter()
        .map(|s| {
            let mut names = Vec::new();
            files(&s.dir, "", 60, &mut names);
            json!({"name": s.name, "description": s.description, "files": names})
        })
        .collect();
    json!({"skills": skills})
}

/// `read_skill`: one file of a skill, `path` relative to the skill's folder (default `SKILL.md`).
pub fn read(dir: &Path, name: &str, path: Option<&str>) -> Result<Value, String> {
    let skill = scan(dir).into_iter().find(|s| s.name == name).ok_or_else(|| format!("no skill `{name}`; `list_skills` shows the ones there are"))?;
    let rel = path.unwrap_or("SKILL.md").trim_start_matches('/');
    // A path that leaves the skill's folder is refused before anything is read.
    if rel.split('/').any(|p| p == ".." || p.is_empty()) || rel.contains('\\') {
        return Err(format!("`{rel}` is not a file of `{name}`"));
    }
    let root = std::fs::canonicalize(&skill.dir).map_err(|e| format!("{}: {e}", skill.dir.display()))?;
    let file = std::fs::canonicalize(skill.dir.join(rel)).map_err(|_| format!("no file `{rel}` in skill `{name}`"))?;
    if !file.starts_with(&root) {
        return Err(format!("`{rel}` is not a file of `{name}`"));
    }
    let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let cut = bytes.len() > MAX_TEXT;
    let text: String = String::from_utf8_lossy(bytes.get(..MAX_TEXT.min(bytes.len())).unwrap_or_default()).into_owned();
    Ok(json!({"skill": name, "path": rel, "text": text, "truncated": cut}))
}

/// The system-prompt index: each skill's name and when to use it. Its text is read on demand.
pub fn prompt_index(dir: &Path) -> Option<String> {
    let skills = scan(dir);
    if skills.is_empty() {
        return None;
    }
    let mut out = String::from(
        "Skills are available to you. Read one before the work it covers, with `read_skill {name}`, \
         and read its files (`reference.md`, scripts, templates) the same way. \
         `list_skills` shows every file.\n",
    );
    for s in &skills {
        out.push_str(&format!("\n- `{}`: {}", s.name, s.description));
    }
    out.push('\n');
    Some(out)
}

/// The two tool definitions, in the same shape as the MCP server's own.
pub fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "list_skills",
            "title": "List skills",
            "description": "The SolidCraft skills installed for this session: each one's name, what it is for, \
                            and the files it holds (SKILL.md, reference.md, scripts, templates).",
            "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false},
            "annotations": {"title": "List skills", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false},
        }),
        json!({
            "name": "read_skill",
            "title": "Read a skill file",
            "description": "Read one file of an installed skill: its SKILL.md by default, or a file it lists \
                            (such as `reference.md`, `scripts/run.py` or `templates/test_design.py`). Read the \
                            skill's SKILL.md first; it says which of its files matter for the task.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "Skill name, as `list_skills` gives it (e.g. \"solidcraft-modeling\")"},
                    "path": {"type": "string", "description": "File within the skill's folder, e.g. \"reference.md\" (default \"SKILL.md\")"},
                },
                "required": ["name"],
                "additionalProperties": false,
            },
            "annotations": {"title": "Read a skill file", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false},
        }),
    ]
}

/// `call_tool` for the two above; `None` when `name` is another tool's.
pub fn call(name: &str, args: &Value, dir: &Path) -> Option<Value> {
    let text = |v: Value| json!({"content": [{"type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default()}]});
    let fail = |e: String| json!({"content": [{"type": "text", "text": e}], "isError": true});
    match name {
        "list_skills" => Some(text(list(dir))),
        "read_skill" => {
            let skill = args.get("name").and_then(Value::as_str).unwrap_or("");
            if skill.is_empty() {
                return Some(fail("read_skill wants `name`, as `list_skills` gives it".into()));
            }
            Some(match read(dir, skill, args.get("path").and_then(Value::as_str)) {
                Ok(v) => text(v),
                Err(e) => fail(e),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(root: &Path, name: &str, desc: &str) {
        let d = root.join(name);
        std::fs::create_dir_all(d.join("scripts")).unwrap();
        std::fs::write(d.join("SKILL.md"), format!("---\nname: {name}\ndescription: {desc}\n---\n\nBody.\n")).unwrap();
        std::fs::write(d.join("reference.md"), "Reference.\n").unwrap();
        std::fs::write(d.join("scripts/run.py"), "print(1)\n").unwrap();
    }

    fn base(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("solidcraft-skills-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_index_lists_each_skill_and_its_files() {
        let d = base("index");
        skill(&d, "solidcraft-modeling", "Use when building a part.");
        let v = list(&d);
        let s = &v["skills"][0];
        assert_eq!(s["name"], "solidcraft-modeling");
        assert_eq!(s["description"], "Use when building a part.");
        let files: Vec<&str> = s["files"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        assert!(files.contains(&"SKILL.md") && files.contains(&"reference.md") && files.contains(&"scripts/run.py"), "{files:?}");
        let prompt = prompt_index(&d).unwrap();
        assert!(prompt.contains("`solidcraft-modeling`: Use when building a part."), "{prompt}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_skill_file_is_read_and_a_path_out_of_the_skill_is_refused() {
        let d = base("read");
        skill(&d, "solidcraft-modeling", "d");
        let v = read(&d, "solidcraft-modeling", None).unwrap();
        assert!(v["text"].as_str().unwrap().contains("Body."));
        let v = read(&d, "solidcraft-modeling", Some("scripts/run.py")).unwrap();
        assert_eq!(v["text"], "print(1)\n");
        assert!(read(&d, "solidcraft-modeling", Some("../../etc/passwd")).is_err());
        assert!(read(&d, "solidcraft-modeling", Some("scripts\\..\\..\\x")).is_err());
        assert!(read(&d, "nope", None).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_tools_answer_unknown_names_with_none_so_mcp_serves_them() {
        let d = base("call");
        skill(&d, "solidcraft-modeling", "d");
        assert!(call("batch", &json!({}), &d).is_none());
        let r = call("read_skill", &json!({"name": "solidcraft-modeling"}), &d).unwrap();
        assert!(r.get("isError").is_none(), "{r}");
        let r = call("read_skill", &json!({"name": "solidcraft-modeling", "path": "../x"}), &d).unwrap();
        assert_eq!(r["isError"], json!(true), "{r}");
        let _ = std::fs::remove_dir_all(&d);
    }
}
