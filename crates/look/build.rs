//! Compiles SolidCraft's look into tables: the icon masters, colour roles, accents and aliases
//! (`icons/design`) and the built-in theme files (`themes`). Every path is inside this crate.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if let Err(e) = icon_data() {
        eprintln!("icon data: {e}");
        std::process::exit(1);
    }
    if let Err(e) = builtin_themes() {
        eprintln!("themes: {e}");
        std::process::exit(1);
    }
}

/// `icons/design`: the SVG masters, `palette.toml` (every colour role in light and dark,
/// and the accents) and `aliases.toml`, compiled into `$OUT_DIR/icon_data.rs` for `icons.rs`.
/// A missing hex, a role an accent does not know or an alias to a missing master fails the build.
fn icon_data() -> Result<(), String> {
    use std::fmt::Write as _;
    let manifest = std::env::var("CARGO_MANIFEST_DIR").map_err(|e| e.to_string())?;
    let root = std::path::Path::new(&manifest).join("icons/design");
    let read = |name: &str| std::fs::read_to_string(root.join(name)).map_err(|e| format!("{name}: {e}"));
    let table = |name: &str| read(name)?.parse::<toml::Table>().map_err(|e| format!("{name}: {e}"));
    let hex = |v: Option<&toml::Value>, what: &str| -> Result<String, String> {
        let s = v.and_then(toml::Value::as_str).ok_or_else(|| format!("{what}: missing hex"))?;
        let ok = s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit());
        if ok { Ok(s.to_ascii_lowercase()) } else { Err(format!("{what}: `{s}` is not #rrggbb")) }
    };
    let rgb = |h: &str| format!("[0x{}, 0x{}, 0x{}]", &h[1..3], &h[3..5], &h[5..7]);
    let palette = table("palette.toml")?;
    let roles = palette.get("roles").and_then(toml::Value::as_table).ok_or("palette.toml: no [roles]")?;
    let names: Vec<&String> = roles.keys().collect();
    let mut out = String::new();
    let _ = writeln!(out, "pub const ROLE_NAMES: &[&str] = &{:?};", names);
    let (mut lights, mut darks, mut light_hex) = (Vec::new(), Vec::new(), Vec::new());
    for (name, role) in roles {
        let l = hex(role.get("light"), &format!("role {name} light"))?;
        let d = hex(role.get("dark"), &format!("role {name} dark"))?;
        lights.push(rgb(&l));
        darks.push(rgb(&d));
        light_hex.push(hex(role.get("source").or_else(|| role.get("light")), &format!("role {name} source"))?);
    }
    let _ = writeln!(out, "pub const ROLE_SOURCE_HEX: &[&str] = &{:?};", light_hex);
    let _ = writeln!(out, "pub const PALETTE: [&[[u8; 3]]; 2] = [&[{}], &[{}]];", lights.join(", "), darks.join(", "));
    let mut accent_names = Vec::new();
    let mut accents = Vec::new();
    if let Some(table) = palette.get("accents").and_then(toml::Value::as_table) {
        for (acc, roles_of) in table {
            let roles_of = roles_of.as_table().ok_or_else(|| format!("accent {acc}: not a table"))?;
            let mut items = Vec::new();
            for (role, v) in roles_of {
                let idx = names.iter().position(|n| *n == role).ok_or_else(|| format!("accent {acc}: unknown role {role}"))?;
                let v = if let Some(link) = v.get("use").and_then(toml::Value::as_str) {
                    roles_of.get(link).ok_or_else(|| format!("accent {acc}: unknown linked role {link}"))?
                } else {
                    v
                };
                let l = hex(v.get("light"), &format!("accent {acc} {role} light"))?;
                let d = hex(v.get("dark"), &format!("accent {acc} {role} dark"))?;
                items.push(format!("({idx}, {}, {})", rgb(&l), rgb(&d)));
            }
            accent_names.push(acc.clone());
            accents.push(format!("&[{}]", items.join(", ")));
        }
    }
    let _ = writeln!(out, "pub const ACCENT_NAMES: &[&str] = &{:?};", accent_names);
    let _ = writeln!(out, "pub const ACCENTS: &[&[(usize, [u8; 3], [u8; 3])]] = &[{}];", accents.join(", "));
    let mut variants = Vec::new();
    if let Some(groups) = palette.get("variants").and_then(toml::Value::as_table) {
        for (vision, group) in groups {
            if !matches!(vision.as_str(), "red-green" | "blue-yellow") {
                return Err(format!("unknown icon colour-vision group {vision}"));
            }
            for (accent, overrides) in group.as_table().ok_or("invalid variant group")? {
                let a = accent_names.iter().position(|n| n == accent).ok_or_else(|| format!("variant: unknown accent {accent}"))?;
                for (role, value) in overrides.as_table().ok_or("invalid variant roles")? {
                    let r = names.iter().position(|n| *n == role).ok_or_else(|| format!("variant: unknown role {role}"))?;
                    let l = hex(value.get("light"), "variant light")?;
                    let d = hex(value.get("dark"), "variant dark")?;
                    variants.push(format!("({vision:?}, {a}, {r}, {}, {})", rgb(&l), rgb(&d)));
                }
            }
        }
    }
    let _ = writeln!(out, "pub const ACCENT_VARIANTS: &[(&str, usize, usize, [u8; 3], [u8; 3])] = &[{}];", variants.join(", "));
    // The masters, by family.
    let mut svgs = Vec::new();
    let mut ids = std::collections::BTreeSet::new();
    let svg_dir = root.join("svg");
    println!("cargo:rerun-if-changed={}", root.display());
    for fam in ["model", "small", "flat", "workspace"] {
        let dir = svg_dir.join(fam);
        let mut files: Vec<_> =
            std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?.filter_map(Result::ok).map(|e| e.path()).collect();
        files.sort();
        for f in files.iter().filter(|f| f.extension().is_some_and(|e| e == "svg")) {
            let id = f.file_stem().and_then(|s| s.to_str()).ok_or("bad file name")?;
            if fam != "small" {
                ids.insert(id.to_owned());
            }
            let abs = f.canonicalize().map_err(|e| e.to_string())?;
            println!("cargo:rerun-if-changed={}", abs.display());
            svgs.push(format!("({id:?}, {fam:?}, include_str!({:?}))", abs.display().to_string()));
        }
    }
    let _ = writeln!(out, "pub const MASTERS: &[(&str, &str, &str)] = &[{}];", svgs.join(", "));
    let aliases = table("aliases.toml")?;
    let mut al = Vec::new();
    if let Some(t) = aliases.get("alias").and_then(toml::Value::as_table) {
        for (alias, v) in t {
            let (master, accent) = match v {
                toml::Value::String(m) => (m.clone(), String::new()),
                toml::Value::Table(t) => (
                    t.get("master").and_then(toml::Value::as_str).ok_or_else(|| format!("alias {alias}: no master"))?.to_string(),
                    t.get("accent").and_then(toml::Value::as_str).unwrap_or("").to_string(),
                ),
                _ => return Err(format!("alias {alias}: expected a string or a table")),
            };
            if !accent.is_empty() && !accent_names.contains(&accent) {
                return Err(format!("alias {alias}: unknown accent {accent}"));
            }
            if !ids.contains(&master) {
                return Err(format!("alias {alias}: unknown master {master}"));
            }
            al.push(format!("({alias:?}, {master:?}, {accent:?})"));
        }
    }
    let _ = writeln!(out, "pub const ALIASES: &[(&str, &str, &str)] = &[{}];", al.join(", "));
    if let Some(t) = aliases.get("alias").and_then(toml::Value::as_table) {
        ids.extend(t.keys().cloned());
    }
    let mut defaults = std::collections::BTreeMap::new();
    if let Some(groups) = palette.get("icon-accents").and_then(toml::Value::as_table) {
        for (accent, icons) in groups {
            if !accent_names.contains(accent) {
                return Err(format!("icon-accents: unknown accent {accent}"));
            }
            for icon in icons.as_array().ok_or("icon-accents: expected icon array")? {
                let icon = icon.as_str().ok_or("icon-accents: expected icon id")?;
                if !ids.contains(icon) || defaults.insert(icon, accent).is_some() {
                    return Err(format!("icon-accents: unknown or repeated icon {icon}"));
                }
            }
        }
    }
    let defaults: Vec<_> = defaults.iter().map(|(id, accent)| format!("({id:?}, {accent:?})")).collect();
    let _ = writeln!(out, "pub const ICON_ACCENTS: &[(&str, &str)] = &[{}];", defaults.join(", "));
    let dest = std::path::Path::new(&std::env::var("OUT_DIR").map_err(|e| e.to_string())?).join("icon_data.rs");
    std::fs::write(dest, out).map_err(|e| e.to_string())
}

/// Embed each theme without a shared registry. Only std is needed to collect the files;
/// theme parsing and ordering use the same validation on native and web targets.
fn builtin_themes() -> Result<(), String> {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").map_err(|e| e.to_string())?;
    let dir = std::path::Path::new(&manifest).join("themes");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_some_and(|e| e == "json") {
            files.push(path);
        }
    }
    files.sort_by(|a, b| a.file_stem().cmp(&b.file_stem()));
    let mut out = String::from("/// The theme files shipped with SolidCraft.\npub const BUILTIN: &[(&str, &str)] = &[\n");
    for path in files {
        let id = path.file_stem().and_then(|s| s.to_str()).ok_or("theme file name is not UTF-8")?;
        let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        // Keep each file's provenance beside its generated entry, never in a shared header.
        let provenance = text
            .lines()
            .find(|line| line.trim_start().starts_with("\"provenance\":"))
            .ok_or_else(|| format!("{}: missing provenance", path.display()))?;
        out += &format!("    ({id:?}, include_str!({:?})), // {}\n", path.display().to_string(), provenance.trim());
    }
    out += "];\n";
    let dest = std::path::Path::new(&std::env::var("OUT_DIR").map_err(|e| e.to_string())?).join("builtin_themes.rs");
    std::fs::write(dest, out).map_err(|e| e.to_string())
}
