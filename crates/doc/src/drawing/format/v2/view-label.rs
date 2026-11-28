//! A drawing's views placed apart from their projection place save a label (`label`, #13044): the
//! view's letter, the words printed with it (`VIEW`, `SCALE`), a name, which parts show, and where it
//! sits from the view's centre. The drawing saves how its views' letters are given (`view_letters`:
//! whether a deleted view's letter is given again, and how many have been given). An older drawing
//! has neither: its projected views placed at a point get their letters here, in id order, and a
//! deleted view's letter is given again. The last release warns about the newer format number,
//! opens what it can read and keeps the rest.
use serde_json::Value;
use solidcraft_geom::drawing_sheet::view_letter;

use crate::DocError;
use crate::drawing::view::{MAX_LETTERS, MAX_VIEWS, ViewLabel, ViewLetters};

pub fn step(v: &mut Value) -> crate::Result<()> {
    let Some(top) = v.as_object_mut() else { return Ok(()) };
    let Some(views) = top.get_mut("views").and_then(Value::as_array_mut) else { return Ok(()) };
    if views.len() > MAX_VIEWS {
        return Err(DocError::Invalid(format!("a drawing has at most {MAX_VIEWS} views")));
    }
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
    // Starting from `ViewLetters::default()` with no letters taken yet, the `i`th view gets
    // `view_letter(i)` directly in O(1) per view instead of rebuilding the used set each step.
    let mut given = ViewLetters::default();
    for (i, (_, at)) in apart.into_iter().enumerate() {
        let letter = view_letter(i)
            .filter(|letter| letter.len() <= MAX_LETTERS)
            .ok_or_else(|| DocError::Invalid("every view letter up to ZZZ has been given".into()))?;
        given.given = u32::try_from(i)
            .ok()
            .and_then(|n| n.checked_add(1))
            .ok_or_else(|| DocError::Invalid("every view letter up to ZZZ has been given".into()))?;
        let label = serde_json::to_value(ViewLabel::new(letter)).map_err(unsaved)?;
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

    #[test]
    fn max_views_upgrade_in_linear_time_and_excess_views_are_rejected() {
        let count = crate::drawing::view::MAX_VIEWS;
        let views: Vec<Value> = (1..=count as u64)
            .map(|id| {
                json!({
                    "id": crate::ids::encode(id),
                    "sheet": "0000000000001",
                    "kind": {"projected": {"parent": "0000000000003", "side": "right"}},
                    "place": {"at": [100.0, 100.0]},
                })
            })
            .collect();
        let mut v = json!({"views": views});
        super::step(&mut v).unwrap();
        assert_eq!(v["views"][0]["label"]["letter"], json!("A"));
        assert_eq!(v["views"][count - 1]["label"]["letter"], json!(solidcraft_geom::drawing_sheet::view_letter(count - 1).unwrap()));
        assert_eq!(v["view_letters"], json!({"reuse": true, "given": count}));

        let mut too_many = json!({"views": vec![json!({"id": "0000000000001"}); count + 1]});
        assert!(super::step(&mut too_many).is_err());
    }
}
