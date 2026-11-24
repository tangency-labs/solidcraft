//! A drawing's views placed apart from their projection place save a label (`label`, #13044): the
//! view's letter, the words printed with it (`VIEW`, `SCALE`), a name, which parts show, and where it
//! sits from the view's centre. The drawing saves how its views' letters are given (`view_letters`:
//! whether a deleted view's letter is given again, and how many have been given). An older drawing
//! has neither: its projected views placed at a point get their letters here, in id order, and a
//! deleted view's letter is given again. The last release warns about the newer format number,
//! opens what it can read and keeps the rest.
use serde_json::Value;

use crate::DocError;
use crate::drawing::view::{ViewLabel, ViewLetters};

pub fn step(v: &mut Value) -> crate::Result<()> {
    let Some(top) = v.as_object_mut() else { return Ok(()) };
    let Some(views) = top.get_mut("views").and_then(Value::as_array_mut) else { return Ok(()) };
    // Placed apart, as `View::needs_letter` reads it: a projected view placed at a point.
    let has = |view: &Value, field: &str, key: &str| view.get(field).and_then(|value| value.get(key)).is_some();
    let mut apart: Vec<(u64, usize)> = views
        .iter()
        .enumerate()
        .filter(|(_, view)| has(view, "kind", "projected") && has(view, "place", "at") && view.get("label").is_none())
        .filter_map(|(at, view)| Some((crate::ids::decode(view.get("id")?.as_str()?)?, at)))
        .collect();
    if apart.is_empty() {
        return Ok(());
    }
    apart.sort_unstable();
    let unsaved = |e: serde_json::Error| DocError::Invalid(format!("view label not saved: {e}"));
    let mut given = ViewLetters::default();
    let mut used: Vec<String> = Vec::new();
    for (_, at) in apart {
        let letter = given.next(used.iter().map(String::as_str))?;
        let label = serde_json::to_value(ViewLabel::new(letter.clone())).map_err(unsaved)?;
        used.push(letter);
        if let Some(view) = views.get_mut(at).and_then(Value::as_object_mut) {
            view.insert("label".into(), label);
        }
    }
    top.insert("view_letters".into(), serde_json::to_value(given).map_err(unsaved)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    /// The projected views placed at a point get letters in id order, whatever order the file lists
    /// them in; a base view and a view lined up with its parent get none; a drawing with no views is
    /// left as it was.
    #[test]
    fn an_older_drawings_views_placed_apart_get_their_letters() {
        let base = json!({"id": "0000000000003", "sheet": "0000000000001", "kind": {"base": {"source": "0000000000002", "view": "front"}}, "place": {"at": [100.0, 100.0]}, "settings": {"scale": "1:2"}});
        let projected = |id: &str, place: serde_json::Value| json!({"id": id, "sheet": "0000000000001", "kind": {"projected": {"parent": "0000000000003", "side": "right"}}, "place": place});
        let mut v = json!({"views": [
            base,
            projected("0000000000009", json!({"at": [300.0, 60.0]})),
            projected("0000000000005", json!({"aligned": 250.0})),
            projected("0000000000007", json!({"at": [300.0, 200.0]})),
        ]});
        super::step(&mut v).unwrap();
        let letters: Vec<Value> = v["views"].as_array().unwrap().iter().map(|view| view["label"]["letter"].clone()).collect();
        assert_eq!(letters, [Value::Null, json!("B"), Value::Null, json!("A")]);
        assert_eq!(v["views"][3]["label"], json!({"letter": "A", "words": "VIEW", "scale_words": "SCALE"}));
        assert_eq!(v["view_letters"], json!({"reuse": true, "given": 2}));
        let mut empty = json!({"name": "Untitled"});
        super::step(&mut empty).unwrap();
        assert_eq!(empty, json!({"name": "Untitled"}));
    }
}
