//! Read-only access to the skill documents shipped with SolidCraft.

use serde_json::{Map, Value};

struct Skill {
    name: &'static str,
    files: &'static [(&'static str, &'static str)],
}

// Exact names only: these are embedded documents, never filesystem paths to open.
const SKILLS: &[Skill] = &[
    Skill {
        name: "solidcraft-checking",
        files: &[
            ("SKILL.md", include_str!("../../../skills/solidcraft-checking/SKILL.md")),
            ("reference/porting.md", include_str!("../../../skills/solidcraft-checking/reference/porting.md")),
        ],
    },
    Skill { name: "solidcraft-editing", files: &[("SKILL.md", include_str!("../../../skills/solidcraft-editing/SKILL.md"))] },
    Skill {
        name: "solidcraft-mechanisms",
        files: &[
            ("SKILL.md", include_str!("../../../skills/solidcraft-mechanisms/SKILL.md")),
            ("reference.md", include_str!("../../../skills/solidcraft-mechanisms/reference.md")),
        ],
    },
    Skill {
        name: "solidcraft-modeling",
        files: &[
            ("SKILL.md", include_str!("../../../skills/solidcraft-modeling/SKILL.md")),
            ("reference.md", include_str!("../../../skills/solidcraft-modeling/reference.md")),
        ],
    },
    Skill {
        name: "solidcraft-reporting",
        files: &[
            ("SKILL.md", include_str!("../../../skills/solidcraft-reporting/SKILL.md")),
            ("reference/report-template.md", include_str!("../../../skills/solidcraft-reporting/reference/report-template.md")),
        ],
    },
    Skill {
        name: "solidcraft-sheetmetal",
        files: &[
            ("SKILL.md", include_str!("../../../skills/solidcraft-sheetmetal/SKILL.md")),
            ("reference.md", include_str!("../../../skills/solidcraft-sheetmetal/reference.md")),
        ],
    },
];

pub(crate) fn get(skill: &str, file: &str) -> Option<&'static str> {
    let entry = SKILLS.iter().find(|s| s.name == skill)?;
    entry.files.iter().find(|(name, _)| *name == file).map(|(_, text)| *text)
}

pub(crate) fn skill_names() -> Vec<String> {
    SKILLS.iter().map(|s| s.name.to_owned()).collect()
}

pub(crate) fn skill_files(skill: &str) -> Vec<String> {
    SKILLS.iter().find(|s| s.name == skill).map(|s| s.files.iter().map(|(name, _)| (*name).to_owned()).collect()).unwrap_or_default()
}

pub(crate) fn all_skill_files() -> Vec<String> {
    let mut files: Vec<String> = SKILLS.iter().flat_map(|s| s.files.iter().map(|(name, _)| (*name).to_owned())).collect();
    files.sort();
    files.dedup();
    files
}

pub(crate) fn read(args: &Map<String, Value>) -> Result<Value, String> {
    let name = args.get("skill").and_then(Value::as_str).ok_or("`skill` is required and must be a string")?;
    let skill = SKILLS
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("Unknown skill. Available skills: {}", SKILLS.iter().map(|s| s.name).collect::<Vec<_>>().join(", ")))?;
    let file = match args.get("file") {
        None | Some(Value::Null) => {
            if skill.files.iter().any(|(name, _)| *name == "reference.md") {
                "reference.md"
            } else {
                "SKILL.md"
            }
        }
        Some(Value::String(file)) => file.as_str(),
        Some(_) => return Err("`file` must be a string".into()),
    };
    skill.files.iter().find(|(name, _)| *name == file).map(|(_, text)| Value::String((*text).into())).ok_or_else(|| {
        format!("Unknown file for {}. Available files: {}", skill.name, skill.files.iter().map(|(name, _)| *name).collect::<Vec<_>>().join(", "))
    })
}
