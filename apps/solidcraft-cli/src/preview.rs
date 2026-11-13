//! What a `snapshot` draws see-through, what it draws in a colour of its own, and where it cuts
//! the model: the `--body`, `--tool` and `--section` options.
//!
//! None of this is a renderer of its own. Each option turns into the command the app itself runs
//! — `appearance.assign` for the Appearance dialog, `inspect.section` for Section Analysis — so
//! a picture of a tool body inside a translucent target is the picture the window would show
//! with the same appearances assigned, and a script can do the same thing without the options.

use serde_json::{Value, json};
use solidcraft_engine::Session;

/// How see-through the bodies around a `--tool` are drawn: enough to read the tool through them,
/// solid enough to keep the shape of the material it meets.
pub const TARGET_OPACITY: f64 = 0.3;

/// A `--tool` body's colour: the hue the viewport's feature preview uses for removed material,
/// so a cut's tool looks in a picture the way the preview looks in the window.
pub const TOOL_COLOR: &str = "#e8542c";

/// The colour of a body with no appearance of its own ([`solidcraft_engine::view::colors::BODY`]),
/// as `appearance.assign` takes it. Giving a body an opacity means giving it a look, and a look
/// carries a colour: this keeps the one it is already drawn in.
fn default_body_hex() -> String {
    let c = solidcraft_engine::view::colors::BODY;
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

/// How one body is drawn: `--body NAME:opacity=0.3,color=#cc3333`. Either key alone is enough;
/// the other keeps what the design already says.
#[derive(Debug, PartialEq)]
pub struct BodyLook {
    pub name: String,
    pub opacity: Option<f64>,
    pub color: Option<String>,
}

/// `NAME:key=value[,key=value]`, or `NAME` on its own (which says nothing and is an error:
/// a body is named here to change how it is drawn).
pub fn parse_body(spec: &str) -> Result<BodyLook, String> {
    let (name, rest) = spec
        .split_once(':')
        .ok_or_else(|| format!("--body `{spec}` must be NAME:key=value (`Target:opacity=0.3`, `Tool:color=#cc3333`, keys separated by commas)"))?;
    let name = name.trim();
    if name.is_empty() {
        return Err(format!("--body `{spec}` names no body"));
    }
    let mut look = BodyLook { name: name.to_owned(), opacity: None, color: None };
    for part in rest.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (key, value) = part.split_once('=').ok_or_else(|| format!("--body {name}: `{part}` must be key=value (opacity, color)"))?;
        let value = value.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            "opacity" => {
                let o: f64 = value.parse().map_err(|_| format!("--body {name}: opacity must be a number 0 to 1, not `{value}`"))?;
                if !o.is_finite() || !(0.0..=1.0).contains(&o) {
                    return Err(format!("--body {name}: opacity must be 0 to 1, not `{value}`"));
                }
                look.opacity = Some(o);
            }
            "color" => {
                let digits = value.strip_prefix('#').filter(|h| h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()));
                let digits = digits.ok_or_else(|| format!("--body {name}: color must be #rrggbb, not `{value}`"))?;
                look.color = Some(format!("#{}", digits.to_ascii_lowercase()));
            }
            other => return Err(format!("--body {name}: unknown key `{other}` (opacity, color)")),
        }
    }
    if look.opacity.is_none() && look.color.is_none() {
        return Err(format!("--body `{spec}` asks for nothing: give opacity, color, or both"));
    }
    Ok(look)
}

/// The `inspect.section` parameters a `--section` spec asks for: `z=27` (the plane through that
/// world coordinate, its normal along +Z), `XY` (a named plane through the origin), either with
/// `,flip` to keep the other side, or the command's own JSON object.
pub fn parse_section(spec: &str) -> Result<Value, String> {
    let spec = spec.trim();
    if spec.starts_with('{') {
        return serde_json::from_str(spec).map_err(|e| format!("--section: not JSON: {e}"));
    }
    let mut parts = spec.split(',').map(str::trim);
    let plane = parts.next().unwrap_or_default();
    let mut flip = false;
    for p in parts {
        match p.to_ascii_lowercase().as_str() {
            "" => {}
            "flip" => flip = true,
            other => return Err(format!("--section: unknown `{other}` (only `flip` follows the plane)")),
        }
    }
    // A named plane through the world origin, as `inspect.section` takes it.
    let mut params = match plane.to_ascii_uppercase().as_str() {
        p @ ("XY" | "XZ" | "YZ") => json!({"plane": p}),
        _ => {
            // AXIS=VALUE: the plane whose normal is that axis, cutting at that world coordinate.
            let (axis, at) = plane.split_once('=').ok_or_else(|| {
                format!("--section `{spec}` must be z=27 (a world coordinate on an axis), XY, XZ, YZ, or the JSON inspect.section takes")
            })?;
            let at: f64 = at.trim().parse().map_err(|_| format!("--section {axis}: `{}` is not a number", at.trim()))?;
            if !at.is_finite() {
                return Err(format!("--section {axis}: `{at}` is not a finite coordinate"));
            }
            // The plane normal to each axis: Z is XY, Y is XZ, X is YZ.
            let named = match axis.trim().to_ascii_lowercase().as_str() {
                "x" => "YZ",
                "y" => "XZ",
                "z" => "XY",
                other => return Err(format!("--section: unknown axis `{other}` (x, y or z)")),
            };
            json!({"plane": named, "at": at})
        }
    };
    if flip {
        params["flip"] = json!(true);
    }
    Ok(params)
}

/// The design's body names, for an error that says which ones there are.
fn body_names(s: &Session) -> Vec<String> {
    s.model.state().bodies.iter().map(|b| b.name.clone()).collect()
}

/// The colour and opacity a body is drawn with now, as `appearance.assign` takes them: its own
/// appearance, else its component's or material's, else the default body colour.
fn current_look(s: &mut Session, name: &str) -> Result<(String, f64), String> {
    let reply = s.execute("appearance.list", &json!({"body": name})).map_err(|e| e.to_string())?;
    let body = reply.get("body");
    let look = body.and_then(|b| b.get("appearance")).filter(|a| !a.is_null());
    let hex = look
        .and_then(|a| a.get("color"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        // No appearance, but an imported colour: `body.color` is that colour, 0..1 per channel.
        .or_else(|| {
            let c = body?.get("color")?.as_array()?;
            let ch = |i: usize| -> Option<u8> { Some((c.get(i)?.as_f64()?.clamp(0.0, 1.0) * 255.0).round() as u8) };
            Some(format!("#{:02x}{:02x}{:02x}", ch(0)?, ch(1)?, ch(2)?))
        })
        .unwrap_or_else(default_body_hex);
    let opacity = look.and_then(|a| a.get("opacity")).and_then(Value::as_f64).filter(|o| o.is_finite()).unwrap_or(1.0);
    Ok((hex, opacity))
}

/// Draw `name` with `look`, keeping whichever of colour and opacity it does not give.
fn assign(s: &mut Session, look: &BodyLook) -> Result<(), String> {
    let name = &look.name;
    if !body_names(s).iter().any(|b| b == name) {
        return Err(format!("no body `{name}` in the design (it has {})", body_list(s)));
    }
    let (hex, opacity) = current_look(s, name)?;
    let params = json!({
        "bodies": [name],
        "color": look.color.clone().unwrap_or(hex),
        "opacity": look.opacity.unwrap_or(opacity),
    });
    s.execute("appearance.assign", &params).map(|_| ()).map_err(|e| format!("drawing `{name}`: {e}"))
}

/// The design's bodies, named, for an error message.
fn body_list(s: &Session) -> String {
    let names = body_names(s);
    if names.is_empty() { "no bodies".to_owned() } else { names.join(", ") }
}

/// Apply the options to the session, in the order a later one overrides an earlier: the `--tool`
/// preview first (it speaks for every body), then each `--body`, then the section.
pub fn apply(s: &mut Session, bodies: &[&str], tools: &[&str], section: Option<&str>) -> Result<(), String> {
    // Everything is read before anything is drawn, so a misspelt option is an error rather than
    // half a picture.
    let looks: Vec<BodyLook> = bodies.iter().map(|spec| parse_body(spec)).collect::<Result<_, _>>()?;
    let section = section.map(parse_section).transpose()?;
    if !tools.is_empty() {
        let names = body_names(s);
        for t in tools {
            if !names.iter().any(|b| b == t) {
                return Err(format!("--tool: no body `{t}` in the design (it has {})", body_list(s)));
            }
        }
        // Everything but the tool see-through, in the colour it already has, so the tool reads
        // against the material it meets. Bodies that share a colour are assigned together: a
        // design of many bodies then costs a command or two rather than one for each.
        let mut around: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
        for name in names.iter().filter(|n| !tools.contains(&n.as_str())) {
            around.entry(current_look(s, name)?.0).or_default().push(name.clone());
        }
        for (hex, sharing_it) in around {
            let params = json!({"bodies": sharing_it, "color": hex, "opacity": TARGET_OPACITY});
            s.execute("appearance.assign", &params).map_err(|e| format!("--tool: {e}"))?;
        }
        // Then the tool itself, solid in the tool colour.
        let params = json!({"bodies": tools, "color": TOOL_COLOR, "opacity": 1.0});
        s.execute("appearance.assign", &params).map_err(|e| format!("--tool: {e}"))?;
    }
    for look in &looks {
        assign(s, look)?;
    }
    if let Some(params) = section {
        s.execute("inspect.section", &params).map_err(|e| format!("--section: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `--body` names a body and says how it is drawn. Either key alone is enough.
    #[test]
    fn a_body_spec_is_a_name_and_the_keys_appearance_assign_takes() {
        let look = |s: &str| parse_body(s).unwrap();
        assert_eq!(look("Target:opacity=0.3"), BodyLook { name: "Target".into(), opacity: Some(0.3), color: None });
        assert_eq!(look("Tool:color=#CC3333"), BodyLook { name: "Tool".into(), opacity: None, color: Some("#cc3333".into()) });
        assert_eq!(look("Tool: opacity = 1 , color = #00ff00 "), BodyLook { name: "Tool".into(), opacity: Some(1.0), color: Some("#00ff00".into()) });
        // A body name with a space in it, as the browser shows them.
        assert_eq!(look("Bracket arm:opacity=0").name, "Bracket arm");
        assert_eq!(look("A:opacity=0").opacity, Some(0.0));
    }

    /// Every way of getting `--body` wrong is an error that says what it takes, never a panic
    /// and never a silently ignored option.
    #[test]
    fn a_bad_body_spec_says_what_it_takes() {
        for (spec, expect) in [
            ("Target", "NAME:key=value"),
            (":opacity=0.3", "names no body"),
            ("Target:", "asks for nothing"),
            ("Target:opacity", "key=value"),
            ("Target:opacity=high", "0 to 1"),
            ("Target:opacity=1.5", "0 to 1"),
            ("Target:opacity=-0.2", "0 to 1"),
            ("Target:opacity=NaN", "0 to 1"),
            ("Target:opacity=inf", "0 to 1"),
            ("Target:color=red", "#rrggbb"),
            ("Target:color=#ccc", "#rrggbb"),
            ("Target:color=#gggggg", "#rrggbb"),
            ("Target:alpha=0.3", "unknown key `alpha`"),
        ] {
            let e = parse_body(spec).err().unwrap_or_else(|| panic!("{spec} was accepted"));
            assert!(e.contains(expect), "{spec}: {e}");
        }
    }

    /// A `--section` spec is the plane `inspect.section` cuts with: an axis and a world
    /// coordinate, a named plane, `flip` for the other side, or the command's own JSON.
    #[test]
    fn a_section_spec_is_the_plane_inspect_section_takes() {
        let p = |s: &str| parse_section(s).unwrap();
        // The plane normal to each axis, cutting at that coordinate.
        assert_eq!(p("z=27"), json!({"plane": "XY", "at": 27.0}));
        assert_eq!(p("y=-5.5"), json!({"plane": "XZ", "at": -5.5}));
        assert_eq!(p("X=0"), json!({"plane": "YZ", "at": 0.0}));
        assert_eq!(p("z=27,flip"), json!({"plane": "XY", "at": 27.0, "flip": true}));
        // A named plane through the origin, and the command's own parameters.
        assert_eq!(p("XY"), json!({"plane": "XY"}));
        assert_eq!(p("yz,flip"), json!({"plane": "YZ", "flip": true}));
        assert_eq!(p("{\"plane\": \"XY\", \"offset\": 4}"), json!({"plane": "XY", "offset": 4}));
    }

    #[test]
    fn a_bad_section_spec_says_what_it_takes() {
        for (spec, expect) in [
            ("27", "z=27"),
            ("w=4", "unknown axis `w`"),
            ("z=high", "is not a number"),
            ("z=inf", "not a finite coordinate"),
            ("z=27,upside-down", "unknown `upside-down`"),
            ("{\"plane\":", "not JSON"),
        ] {
            let e = parse_section(spec).err().unwrap_or_else(|| panic!("{spec} was accepted"));
            assert!(e.contains(expect), "{spec}: {e}");
        }
    }

    /// The tool colour is the one the options document, in the form `appearance.assign` takes.
    #[test]
    fn the_tool_colour_is_a_hex_colour_the_command_takes() {
        assert_eq!(parse_body(&format!("Tool:color={TOOL_COLOR}")).unwrap().color, Some(TOOL_COLOR.to_owned()));
        assert!((0.0..1.0).contains(&TARGET_OPACITY), "a target must be see-through: {TARGET_OPACITY}");
        assert_eq!(default_body_hex(), "#b0bac6");
    }
}
