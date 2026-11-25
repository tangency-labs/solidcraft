//! `cargo xtask skills`: keeps the router (AGENTS.md) and the two skill folders honest, so agents
//! are never sent to a file, command or skill that is gone. [`SKILL_DIRS`]: `.agents/skills/` holds
//! the dev skills (working on the code), `skills/` the in-app skills (modelling through the MCP
//! server; some are compiled into the app).
//!
//! - Every path an AGENTS.md, README or SKILL.md names (in backticks, or as a Markdown link) exists.
//!   Only paths that start at a top-level entry of the repository are checked; `plan/` (local
//!   only) and `target*` are skipped, as is anything with a placeholder (`<N>`, `*`, `$SKILL`).
//! - Every `cargo xtask <name>` names a command of this tool.
//! - Every `skills/<name>` and `.agents/skills/<name>` names a folder that exists.
//! - The router table in AGENTS.md lists exactly the skills that have a SKILL.md in either folder,
//!   each folder's README table lists exactly its own skills, a skill name is in one folder only,
//!   and each SKILL.md has a front matter `name` equal to its folder and a `description`.
//! - AGENTS.md stays a router (at most [`ROUTER_MAX_CHARS`]) and a new skill stays short
//!   (at most [`SKILL_MAX_CHARS`]); the skills in [`LONG_SKILLS`] predate the cap.
//! - `.agents/skills` is a real folder, and `.claude/skills` the relative symlink
//!   `../.agents/skills` ([`DISCOVERY_LINK`]), so a coding assistant that looks in either finds the
//!   dev skills and nothing else. A checkout without symlinks (Windows with `core.symlinks=false`)
//!   has a text file holding the target instead: that passes with a hint.

use std::collections::BTreeSet;
use std::path::Path;

/// 5600, not 5000, since the router lists every skill on its own line with when to use it.
pub const ROUTER_MAX_CHARS: usize = 5600;
pub const SKILL_MAX_CHARS: usize = 3600;
/// Skills written before the size cap (the modelling skills carry their command shapes).
const LONG_SKILLS: &[&str] =
    &["solidcraft-checking", "solidcraft-contributing", "solidcraft-editing", "solidcraft-mechanisms", "solidcraft-modeling", "solidcraft-reporting"];
const SKIPPED_ROOTS: &[&str] = &["plan", "target", "target-wasm"];
/// The skill folders: the dev skills, then the in-app skills.
pub const SKILL_DIRS: [&str; 2] = [".agents/skills", "skills"];
/// The discovery link and its target, relative to the link's folder: `.agents/skills/` is the one source.
pub const DISCOVERY_LINK: (&str, &str) = (".claude/skills", "../.agents/skills");

/// A file that is checked: its path from the repository root and its text.
pub struct Doc {
    pub path: String,
    pub text: String,
}

/// What the check needs to know about the repository.
pub struct Repo<'a> {
    /// Whether a path from the repository root exists (file or folder).
    pub exists: &'a dyn Fn(&str) -> bool,
    /// The top-level entries of the repository.
    pub roots: &'a [String],
    /// The commands of this tool.
    pub commands: &'a [String],
}

pub fn run(root: &Path) -> Result<(), String> {
    let mut docs = Vec::new();
    // A Windows checkout (core.autocrlf) has CRLF line endings: check the text as Git stores it (#7281).
    let read = |rel: &str| std::fs::read_to_string(root.join(rel)).map(|t| t.replace("\r\n", "\n")).map_err(|e| format!("{rel}: {e}"));
    docs.push(Doc { path: "AGENTS.md".into(), text: read("AGENTS.md")? });
    let mut names = Vec::new();
    for dir in SKILL_DIRS {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(root.join(dir)).map_err(|e| format!("{dir}: {e}"))?.filter_map(Result::ok) {
            if entry.path().join("SKILL.md").is_file() {
                found.push(entry.file_name().to_string_lossy().to_string());
            }
        }
        found.sort();
        for name in found {
            let rel = format!("{dir}/{name}/SKILL.md");
            docs.push(Doc { text: read(&rel)?, path: rel });
            names.push(name);
        }
        let readme = format!("{dir}/README.md");
        docs.push(Doc { text: read(&readme)?, path: readme });
    }
    let roots: Vec<String> = std::fs::read_dir(root)
        .map_err(|e| format!("{}: {e}", root.display()))?
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    let commands = xtask_commands(crate::USAGE);
    let exists = |p: &str| root.join(p).exists();
    let mut problems = check(&docs, &Repo { exists: &exists, roots: &roots, commands: &commands });
    problems.extend(dev_folder(root));
    problems.extend(discovery_link(root));
    if problems.is_empty() {
        println!("skills: AGENTS.md and {} skills are consistent", names.len());
        Ok(())
    } else {
        Err(format!("{} problem(s) in AGENTS.md, skills/ and .agents/skills/:\n  {}", problems.len(), problems.join("\n  ")))
    }
}

/// What is wrong with the dev skill folder: it must be a real folder, not a link.
pub fn dev_folder(root: &Path) -> Option<String> {
    let dir = SKILL_DIRS[0];
    match std::fs::symlink_metadata(root.join(dir)) {
        Ok(meta) if meta.is_dir() => None,
        Ok(_) => Some(format!("`{dir}` must be a real folder (it holds the dev skills), not a link or a file")),
        Err(_) => Some(format!("`{dir}` is missing")),
    }
}

/// What is wrong with the `.claude/skills` link, if anything.
pub fn discovery_link(root: &Path) -> Option<String> {
    let (link, target) = DISCOVERY_LINK;
    let path = root.join(link);
    match std::fs::symlink_metadata(&path) {
        Err(_) => Some(format!("`{link}` is missing: it is the symlink `{target}` (git: `ln -s {target} {link}`)")),
        Ok(meta) if meta.file_type().is_symlink() => {
            let points = std::fs::read_link(&path).ok().map(|t| t.to_string_lossy().replace('\\', "/"));
            if points.as_deref() != Some(target) {
                return Some(format!("`{link}` points to {points:?}, not `{target}`"));
            }
            let resolved = std::fs::canonicalize(&path).ok();
            let dev = std::fs::canonicalize(root.join(SKILL_DIRS[0])).ok();
            if resolved.is_none() || resolved != dev {
                return Some(format!("`{link}` does not resolve to `{}/`", SKILL_DIRS[0]));
            }
            None
        }
        Ok(meta) if meta.is_file() => {
            // A checkout with core.symlinks=false writes the link as a file holding its target.
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            if text.trim() == target {
                println!("skills: `{link}` is a plain file here (symlinks are off in this checkout); see AGENTS.md, \"Skills\"");
                None
            } else {
                Some(format!("`{link}` is a file, not the symlink `{target}`"))
            }
        }
        Ok(_) => Some(format!("`{link}` is a folder, not the symlink `{target}`: `{}/` is the one source", SKILL_DIRS[0])),
    }
}

/// A SKILL.md path as (its folder from the root, its name), when it is in one of [`SKILL_DIRS`].
fn skill_of(path: &str) -> Option<(String, String)> {
    let folder = path.strip_suffix("/SKILL.md")?;
    let (dir, name) = folder.rsplit_once('/')?;
    (SKILL_DIRS.contains(&dir) && !name.is_empty()).then(|| (folder.to_owned(), name.to_owned()))
}

/// The command names in the usage text: lines of the form `  name   what it does`.
pub fn xtask_commands(usage: &str) -> Vec<String> {
    usage
        .lines()
        .filter(|l| l.starts_with("  ") && !l.starts_with("   "))
        .filter_map(|l| l.split_whitespace().next())
        .filter(|w| w.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
        .map(str::to_owned)
        .collect()
}

pub fn check(docs: &[Doc], repo: &Repo) -> Vec<String> {
    let mut problems = Vec::new();
    let mut skill_dirs = BTreeSet::new();
    let mut seen = BTreeSet::new();
    for d in docs {
        if let Some((folder, name)) = skill_of(&d.path) {
            if !seen.insert(name.clone()) {
                problems.push(format!("{folder}: a skill named `{name}` is also in the other skill folder"));
            }
            skill_dirs.insert(folder);
        }
    }
    for d in docs {
        for t in path_mentions(&d.text) {
            if resolve(&d.path, &t, repo).is_some_and(|candidates| !candidates.iter().any(|c| (repo.exists)(c))) {
                problems.push(format!("{}: `{t}` does not exist", d.path));
            }
        }
        for name in command_mentions(&d.text) {
            if !repo.commands.contains(&name) {
                problems.push(format!("{}: `cargo xtask {name}` is not an xtask command", d.path));
            }
        }
        for folder in skill_mentions(&d.text) {
            if !(repo.exists)(&folder) {
                problems.push(format!("{}: `{folder}` does not exist", d.path));
            }
        }
        let chars = d.text.chars().count();
        if d.path == "AGENTS.md" {
            if chars > ROUTER_MAX_CHARS {
                problems.push(format!("AGENTS.md is {chars} characters; a router stays under {ROUTER_MAX_CHARS}: move detail into a skill"));
            }
        } else if let Some((_, name)) = skill_of(&d.path) {
            let name = name.as_str();
            if chars > SKILL_MAX_CHARS && !LONG_SKILLS.contains(&name) {
                problems.push(format!("{}: {chars} characters; a skill stays under {SKILL_MAX_CHARS}: split it", d.path));
            }
            match front_matter(&d.text) {
                None => problems.push(format!("{}: no front matter with name and description", d.path)),
                Some((n, desc)) => {
                    if n != name {
                        problems.push(format!("{}: front matter name `{n}` is not the folder name `{name}`", d.path));
                    }
                    if desc.trim().is_empty() {
                        problems.push(format!("{}: front matter has no description", d.path));
                    }
                }
            }
        }
    }
    if let Some(router) = docs.iter().find(|d| d.path == "AGENTS.md") {
        let routed = router_skills(&router.text);
        for folder in skill_dirs.difference(&routed) {
            problems.push(format!("AGENTS.md: the router table does not list {folder}/SKILL.md"));
        }
        for folder in routed.difference(&skill_dirs) {
            problems.push(format!("AGENTS.md: the router table lists {folder}/SKILL.md, which has no SKILL.md"));
        }
        if routed.is_empty() {
            problems.push("AGENTS.md: no router table (rows naming `skills/<name>/SKILL.md`)".into());
        }
    }
    // Each folder's README lists exactly its own skills, linked as `](<name>/SKILL.md)`.
    for dir in SKILL_DIRS {
        let readme = format!("{dir}/README.md");
        let Some(doc) = docs.iter().find(|d| d.path == readme) else { continue };
        let listed = readme_skills(&doc.text);
        let here: BTreeSet<String> = skill_dirs.iter().filter_map(|f| f.strip_prefix(&format!("{dir}/")).map(str::to_owned)).collect();
        for name in here.difference(&listed) {
            problems.push(format!("{readme}: the table does not list {name}"));
        }
        for name in listed.difference(&here) {
            problems.push(format!("{readme}: the table lists {name}, which is not in {dir}/"));
        }
    }
    problems
}

/// The skill folders named in table rows (`| ... | `<skill dir>/<name>/SKILL.md` |`).
pub fn router_skills(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in text.lines().filter(|l| l.trim_start().starts_with('|')) {
        for span in code_spans(line) {
            if let Some((folder, _)) = skill_of(span) {
                out.insert(folder);
            }
        }
    }
    out
}

/// The skills a README's table links to (`| [`name`](name/SKILL.md) | ... |`).
pub fn readme_skills(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in text.lines().filter(|l| l.trim_start().starts_with('|')) {
        let mut rest = line;
        while let Some(i) = rest.find("](") {
            let after = &rest[i + 2..];
            if let Some(name) = after.split(')').next().and_then(|t| t.strip_suffix("/SKILL.md"))
                && !name.contains('/')
                && !name.is_empty()
            {
                out.insert(name.to_owned());
            }
            rest = after;
        }
    }
    out
}

/// The `name` and `description` of a `---` front matter block.
pub fn front_matter(text: &str) -> Option<(String, String)> {
    let rest = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n"))?;
    let block = rest.split("\n---").next()?;
    let get =
        |key: &str| block.lines().find_map(|l| l.strip_prefix(key).and_then(|v| v.strip_prefix(':'))).map(|v| v.trim().trim_matches('"').to_owned());
    Some((get("name")?, get("description")?))
}

fn code_spans(line: &str) -> Vec<&str> {
    line.split('`').skip(1).step_by(2).collect()
}

/// Backticked spans and Markdown link targets that look like paths.
pub fn path_mentions(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        out.extend(code_spans(line).into_iter().map(str::to_owned));
        let mut rest = line;
        while let Some(i) = rest.find("](") {
            let after = &rest[i + 2..];
            if let Some(end) = after.find(')') {
                out.push(after[..end].to_owned());
            }
            rest = after;
        }
    }
    out
}

/// The repository path a mention stands for, or `None` when it is not a checkable path.
fn resolve(from: &str, mention: &str, repo: &Repo) -> Option<Vec<String>> {
    let m = mention.trim();
    if m.is_empty() || m.contains("://") || m.starts_with('#') || m.starts_with('~') || m.starts_with('/') || m.starts_with("mailto:") {
        return None;
    }
    if m.chars().any(|c| matches!(c, '*' | '<' | '>' | '$' | '{' | '}' | '…' | ' ' | '|' | '(' | ')' | '=')) {
        return None;
    }
    let m = m.split('#').next().unwrap_or(m);
    // `path:12` and `path:12-30` point into a file.
    let m = match m.rsplit_once(':') {
        Some((p, n)) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit() || c == '-') => p,
        _ => m,
    };
    let m = m.trim_end_matches('/');
    let dir = from.rsplit_once('/').map_or("", |(d, _)| d);
    let from_file = normalize(&format!("{dir}/{m}"));
    let candidates = if m.starts_with("./") || m.starts_with("../") {
        vec![from_file?]
    } else {
        // A bare mention is from the repository root (or the file's folder, for a skill's
        // own `scripts/`), and only when its first folder is one of the repository's.
        if !m.contains('/') {
            return None;
        }
        let first = m.split('/').next()?;
        if SKIPPED_ROOTS.contains(&first) || first.starts_with("target") || !repo.roots.iter().any(|r| r == first) {
            return None;
        }
        let mut v = vec![normalize(m)?];
        v.extend(from_file);
        v
    };
    let first = candidates[0].split('/').next()?;
    if SKIPPED_ROOTS.contains(&first) || first.starts_with("target") || !repo.roots.iter().any(|r| r == first) {
        return None;
    }
    Some(candidates)
}

fn normalize(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

/// `cargo xtask <name>` anywhere in the text.
pub fn command_mentions(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("cargo xtask ") {
        let after = &rest[i + "cargo xtask ".len()..];
        let name: String = after.chars().take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-').collect();
        if !name.is_empty() {
            out.push(name);
        }
        rest = after;
    }
    out
}

/// `skills/<name>` and `.agents/skills/<name>` anywhere in the text, as folders from the root (a
/// placeholder such as `skills/<name>` is not a name).
pub fn skill_mentions(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    let mut consumed = 0;
    while let Some(i) = rest.find("skills/") {
        let at = consumed + i;
        let after = &rest[i + "skills/".len()..];
        let name: String = after.chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')).collect();
        let name = name.trim_end_matches('.').to_owned();
        let head = &text[..at];
        let path_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '/' | '-' | '_' | '~');
        let folder = if let Some(h) = head.strip_suffix(".agents/") {
            // `.agents/skills/<name>`, but not the tail of a longer path.
            h.chars().next_back().is_none_or(|c| !path_char(c)).then(|| format!(".agents/skills/{name}"))
        } else {
            // Not the tail of another path such as `.claude/skills/` or `xskills/`.
            head.chars().next_back().is_none_or(|c| !path_char(c)).then(|| format!("skills/{name}"))
        };
        if let Some(f) = folder.filter(|_| !name.is_empty()) {
            out.push(f);
        }
        consumed = at + "skills/".len();
        rest = after;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const USAGE: &str = "usage: cargo xtask <command>\n\ncommands:\n  layers          enforce\n  lists [--fix]   check\n  ci              all\n                  continued line\n";

    fn doc(path: &str, text: &str) -> Doc {
        Doc { path: path.into(), text: text.into() }
    }

    fn problems(docs: &[Doc], files: &[&str]) -> Vec<String> {
        let files: Vec<String> = files.iter().map(|s| s.to_string()).collect();
        let exists = |p: &str| files.iter().any(|f| f == p || f.starts_with(&format!("{p}/")));
        let roots = vec!["AGENTS.md".to_string(), ".agents".into(), "crates".into(), "docs".into(), "skills".into(), "plan".into()];
        let commands = xtask_commands(USAGE);
        check(docs, &Repo { exists: &exists, roots: &roots, commands: &commands })
    }

    const SKILL: &str = "---\nname: solidcraft-a\ndescription: Use when a.\n---\n# A\n";

    fn router(table: &str) -> Doc {
        doc("AGENTS.md", &format!("# Router\n\n| Working on | Read |\n|---|---|\n{table}\n"))
    }

    #[cfg(unix)]
    #[test]
    fn the_claude_link_must_be_the_relative_symlink_to_the_dev_skills() {
        let root = std::env::temp_dir().join(format!("xtask-skills-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".agents/skills")).unwrap();
        std::fs::create_dir_all(root.join("skills")).unwrap();
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        let path = root.join(".claude/skills");
        assert!(discovery_link(&root).unwrap().contains("missing"));
        std::os::unix::fs::symlink("../.agents/skills", &path).unwrap();
        assert_eq!(discovery_link(&root), None);
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, "../.agents/skills").unwrap();
        assert_eq!(discovery_link(&root), None, "a checkout without symlinks passes");
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink("../skills", &path).unwrap();
        assert!(discovery_link(&root).unwrap().contains("points to"), "the in-app skills are not dev skills");
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir_all(&path).unwrap();
        assert!(discovery_link(&root).unwrap().contains("is a folder"));
        // The dev folder itself is real, never a link.
        assert_eq!(dev_folder(&root), None);
        std::fs::remove_dir_all(root.join(".agents/skills")).unwrap();
        std::os::unix::fs::symlink("../skills", root.join(".agents/skills")).unwrap();
        assert!(dev_folder(&root).unwrap().contains("real folder"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dev_and_in_app_skills_are_routed_listed_and_named_once() {
        let dev = SKILL.replace("-a", "-d");
        let docs = [
            router("| a | `skills/solidcraft-a/SKILL.md` |\n| d | `.agents/skills/solidcraft-d/SKILL.md` |"),
            doc("skills/solidcraft-a/SKILL.md", SKILL),
            doc(".agents/skills/solidcraft-d/SKILL.md", &format!("{dev}See `.agents/skills/solidcraft-d/SKILL.md` and skills/solidcraft-a.")),
            doc("skills/README.md", "| [`solidcraft-a`](solidcraft-a/SKILL.md) | a |\n"),
            doc(".agents/skills/README.md", "| [`solidcraft-d`](solidcraft-d/SKILL.md) | d |\n"),
        ];
        let files = ["skills/solidcraft-a/SKILL.md", ".agents/skills/solidcraft-d/SKILL.md", "skills/README.md", ".agents/skills/README.md"];
        assert_eq!(problems(&docs, &files), Vec::<String>::new());
        // A dev skill cited at its old place, a README that misses one, and one name in both folders.
        let docs = [
            router(
                "| a | `skills/solidcraft-a/SKILL.md` |\n| d | `.agents/skills/solidcraft-d/SKILL.md` |\n| a2 | `.agents/skills/solidcraft-a/SKILL.md` |",
            ),
            doc("skills/solidcraft-a/SKILL.md", "---\nname: solidcraft-a\ndescription: Use when a.\n---\nSee skills/solidcraft-d.\n"),
            doc(".agents/skills/solidcraft-d/SKILL.md", &dev),
            doc(".agents/skills/solidcraft-a/SKILL.md", SKILL),
            doc("skills/README.md", "| [`solidcraft-a`](solidcraft-a/SKILL.md) | a |\n"),
            doc(".agents/skills/README.md", "| [`solidcraft-d`](solidcraft-d/SKILL.md) | d |\n"),
        ];
        let p = problems(&docs, &[&files[..], &[".agents/skills/solidcraft-a/SKILL.md"]].concat());
        assert!(p.contains(&"skills/solidcraft-a/SKILL.md: `skills/solidcraft-d` does not exist".to_string()), "{p:?}");
        assert!(p.contains(&".agents/skills/README.md: the table does not list solidcraft-a".to_string()), "{p:?}");
        assert!(p.iter().any(|m| m.contains("also in the other skill folder")), "{p:?}");
    }

    #[test]
    fn commands_come_from_the_usage_text() {
        assert_eq!(xtask_commands(USAGE), ["layers", "lists", "ci"]);
    }

    #[test]
    fn a_consistent_router_passes() {
        let docs = [router("| a | `skills/solidcraft-a/SKILL.md` |"), doc("skills/solidcraft-a/SKILL.md", SKILL)];
        assert_eq!(problems(&docs, &["skills/solidcraft-a/SKILL.md"]), Vec::<String>::new());
    }

    #[test]
    fn a_missing_path_is_reported_and_placeholders_are_not() {
        let text = "See `docs/gone.md`, `docs/here.md`, `crates/doc/src/format/v<N>/`, `crates/*/x.rs`, `plan/STATUS.md`, `~/.claude/skills/x`, `src/lib.rs`, `lib.rs`.";
        let docs = [router("| a | `skills/solidcraft-a/SKILL.md` |"), doc("skills/solidcraft-a/SKILL.md", &format!("{SKILL}{text}"))];
        let p = problems(&docs, &["skills/solidcraft-a/SKILL.md", "docs/here.md"]);
        assert_eq!(p, ["skills/solidcraft-a/SKILL.md: `docs/gone.md` does not exist"]);
    }

    #[test]
    fn links_resolve_from_the_file_and_fences_are_skipped() {
        let text = "[ok](../../docs/here.md#top) [bad](../../docs/no.md) [web](https://x.io/docs/no.md)\n```\n`docs/in-fence.md`\n```\n";
        let docs = [router("| a | `skills/solidcraft-a/SKILL.md` |"), doc("skills/solidcraft-a/SKILL.md", &format!("{SKILL}{text}"))];
        let p = problems(&docs, &["skills/solidcraft-a/SKILL.md", "docs/here.md"]);
        assert_eq!(p, ["skills/solidcraft-a/SKILL.md: `../../docs/no.md` does not exist"]);
    }

    #[test]
    fn a_skills_own_relative_path_resolves_from_its_folder() {
        let docs = [
            router("| a | `skills/solidcraft-a/SKILL.md` |"),
            doc("skills/solidcraft-a/SKILL.md", &format!("{SKILL}`docs/scripts/x.py` `docs/other/y.py`")),
        ];
        let p = problems(&docs, &["skills/solidcraft-a/SKILL.md", "skills/solidcraft-a/docs/scripts/x.py"]);
        assert_eq!(p, ["skills/solidcraft-a/SKILL.md: `docs/other/y.py` does not exist"]);
    }

    #[test]
    fn a_line_number_suffix_is_ignored() {
        let docs = [router("| a | `skills/solidcraft-a/SKILL.md` |"), doc("skills/solidcraft-a/SKILL.md", &format!("{SKILL}`docs/here.md:12-30`"))];
        assert!(problems(&docs, &["skills/solidcraft-a/SKILL.md", "docs/here.md"]).is_empty());
    }

    #[test]
    fn an_unknown_xtask_command_is_reported() {
        let docs = [
            router("| a | `skills/solidcraft-a/SKILL.md` |"),
            doc("skills/solidcraft-a/SKILL.md", &format!("{SKILL}Run `cargo xtask lists --fix` then `cargo xtask nope`.")),
        ];
        let p = problems(&docs, &["skills/solidcraft-a/SKILL.md"]);
        assert_eq!(p, ["skills/solidcraft-a/SKILL.md: `cargo xtask nope` is not an xtask command"]);
    }

    #[test]
    fn a_skill_that_does_not_exist_is_reported() {
        let docs =
            [router("| a | `skills/solidcraft-a/SKILL.md` |\n| b | `skills/solidcraft-b/SKILL.md` |"), doc("skills/solidcraft-a/SKILL.md", SKILL)];
        let p = problems(&docs, &["skills/solidcraft-a/SKILL.md"]);
        assert!(p.contains(&"AGENTS.md: `skills/solidcraft-b` does not exist".to_string()), "{p:?}");
        assert!(p.contains(&"AGENTS.md: the router table lists skills/solidcraft-b/SKILL.md, which has no SKILL.md".to_string()), "{p:?}");
    }

    #[test]
    fn a_skill_missing_from_the_table_is_reported() {
        let docs = [
            router("| a | `skills/solidcraft-a/SKILL.md` |"),
            doc("skills/solidcraft-a/SKILL.md", SKILL),
            doc("skills/solidcraft-c/SKILL.md", &SKILL.replace("-a", "-c")),
        ];
        let p = problems(&docs, &["skills/solidcraft-a/SKILL.md", "skills/solidcraft-c/SKILL.md"]);
        assert_eq!(p, ["AGENTS.md: the router table does not list skills/solidcraft-c/SKILL.md"]);
    }

    #[test]
    fn prose_mentions_of_skills_outside_the_table_do_not_route() {
        let docs = [doc("AGENTS.md", "Install into `~/.claude/skills/` or see skills/solidcraft-a.\n"), doc("skills/solidcraft-a/SKILL.md", SKILL)];
        let p = problems(&docs, &["skills/solidcraft-a/SKILL.md"]);
        assert_eq!(
            p,
            [
                "AGENTS.md: the router table does not list skills/solidcraft-a/SKILL.md",
                "AGENTS.md: no router table (rows naming `skills/<name>/SKILL.md`)"
            ]
        );
    }

    #[test]
    fn front_matter_must_match_the_folder() {
        let docs =
            [router("| a | `skills/solidcraft-a/SKILL.md` |"), doc("skills/solidcraft-a/SKILL.md", "---\nname: other\ndescription: \"\"\n---\n")];
        let p = problems(&docs, &["skills/solidcraft-a/SKILL.md"]);
        assert_eq!(
            p,
            [
                "skills/solidcraft-a/SKILL.md: front matter name `other` is not the folder name `solidcraft-a`",
                "skills/solidcraft-a/SKILL.md: front matter has no description"
            ]
        );
        assert_eq!(front_matter("no front matter"), None);
        assert_eq!(
            front_matter("---\r\nname: solidcraft-a\r\ndescription: Windows checkout\r\n---\r\n"),
            Some(("solidcraft-a".into(), "Windows checkout".into()))
        );
    }

    #[test]
    fn sizes_are_capped_except_for_the_older_skills() {
        let long = "x".repeat(SKILL_MAX_CHARS);
        let docs = [
            doc("AGENTS.md", &format!("| `skills/solidcraft-a/SKILL.md` |{}", "y".repeat(ROUTER_MAX_CHARS))),
            doc("skills/solidcraft-a/SKILL.md", &format!("{SKILL}{long}")),
            doc("skills/solidcraft-modeling/SKILL.md", &format!("{}{long}", SKILL.replace("-a", "-modeling"))),
        ];
        let p = problems(&docs, &["skills/solidcraft-a/SKILL.md", "skills/solidcraft-modeling/SKILL.md"]);
        assert!(p.iter().any(|m| m.starts_with("AGENTS.md is ")), "{p:?}");
        assert!(p.iter().any(|m| m.starts_with("skills/solidcraft-a/SKILL.md: ") && m.contains("split it")), "{p:?}");
        assert!(!p.iter().any(|m| m.contains("solidcraft-modeling/SKILL.md: ") && m.contains("split it")), "{p:?}");
    }

    /// A SKILL.md filled to the size cap, read from a CRLF checkout: its front matter is found
    /// and the line endings don't count towards the cap (#7281).
    #[test]
    fn a_crlf_checkout_is_checked_as_git_stores_it() {
        let root = std::env::temp_dir().join(format!("xtask-skills-crlf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("skills/solidcraft-a")).unwrap();
        let lines = "x\n".repeat((SKILL_MAX_CHARS - SKILL.len()) / 2);
        let skill = format!("{SKILL}{lines}{}", "x".repeat(SKILL_MAX_CHARS - SKILL.len() - lines.len()));
        let crlf = |t: &str| t.replace('\n', "\r\n");
        std::fs::write(root.join("AGENTS.md"), crlf("# Router\n\n| a | `skills/solidcraft-a/SKILL.md` |\n")).unwrap();
        std::fs::write(root.join("skills/solidcraft-a/SKILL.md"), crlf(&skill)).unwrap();
        std::fs::write(root.join("skills/README.md"), crlf("| [`solidcraft-a`](solidcraft-a/SKILL.md) | a |\n")).unwrap();
        std::fs::create_dir_all(root.join(".agents/skills")).unwrap();
        std::fs::write(root.join(".agents/skills/README.md"), crlf("# Dev skills\n")).unwrap();
        // The discovery link as a checkout without symlinks writes it.
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::write(root.join(".claude/skills"), "../.agents/skills").unwrap();
        let result = run(&root);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn mentions_are_found() {
        assert_eq!(command_mentions("run cargo xtask ci, cargo xtask lists --fix; `cargo xtask`"), ["ci", "lists"]);
        assert_eq!(skill_mentions("skills/a-b/SKILL.md, ~/.claude/skills/x, skills/<name>, skills/README.md."), ["skills/a-b", "skills/README.md"]);
        assert_eq!(
            skill_mentions("`.agents/skills/solidcraft-x/SKILL.md`, x.agents/skills/y, skills/z"),
            [".agents/skills/solidcraft-x", "skills/z"]
        );
    }

    /// The real AGENTS.md and skills pass, so a broken citation fails `cargo test -p xtask`.
    #[test]
    fn this_repository_passes() {
        if let Err(e) = run(&crate::root()) {
            panic!("{e}");
        }
    }
}
