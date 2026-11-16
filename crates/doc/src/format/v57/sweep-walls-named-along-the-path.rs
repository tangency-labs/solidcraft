//! A sweep along a path of several curves names every wall it makes along each curve of the path
//! by the profile curve the wall is swept from, from the sweep's own history. Before, only the
//! walls half way along the path had that history; the others were named by the profile curve
//! nearest them once turned back into the profile's plane, a guess that could name a wall after
//! a curve it is not swept from and change when the profile changed size (#17229). The
//! guessed names are gone, so a sketch link to a vertex, an edge or a face of such a sweep that
//! saved one of them (or a name built on one, a later feature's face named beside it) no longer
//! keeps that name: the link is found by its point when the design opens, as a link saved
//! without a name is. A vertex or an edge link is marked (`name_dropped`), and the session that
//! opens the design stores the name of the vertex or edge it was found on there, before any
//! change can move it; a face link takes its name when its sketch is next edited. A link a
//! sketch's plane was placed by (`auto_face`) keeps its name. The last release warns about the
//! newer format number and opens the design, the links being found by their points.
use serde_json::Value;

pub fn step(v: &mut Value) -> crate::Result<()> {
    let Some(Value::Array(features)) = v.get_mut("features") else {
        return Ok(());
    };
    // The sweeps along more than one curve, by id.
    let sweeps: Vec<u64> = features
        .iter()
        .filter(|f| f.get("type").and_then(Value::as_str) == Some("sweep"))
        .filter(|f| ["path", "path_edges"].iter().any(|k| f.get(*k).and_then(Value::as_array).is_some_and(|p| p.len() > 1)))
        .filter_map(|f| f.get("id").and_then(Value::as_u64))
        .collect();
    if sweeps.is_empty() {
        return Ok(());
    }
    for feature in features.iter_mut() {
        let Some(Value::Array(links)) = feature.pointer_mut("/sketch/links") else { continue };
        for link in links.iter_mut() {
            let Value::Object(link) = link else { continue };
            let placed = link.get("auto_face").and_then(Value::as_bool) == Some(true);
            for key in ["vertex_name", "edge_name", "face_name"] {
                if key == "face_name" && placed {
                    continue;
                }
                if link.get(key).and_then(Value::as_str).is_some_and(|name| sweeps.iter().any(|id| names_face_of(name, *id))) {
                    link.remove(key);
                    if key != "face_name" {
                        link.insert("name_dropped".into(), Value::Bool(true));
                    }
                }
            }
        }
    }
    Ok(())
}

/// Whether the persistent name `name` holds a face of feature `id` (`F<id>:`).
fn names_face_of(name: &str, id: u64) -> bool {
    let tag = format!("F{id}:");
    name.match_indices(&tag).any(|(at, _)| !name.get(..at).and_then(|before| before.chars().next_back()).is_some_and(|c| c.is_ascii_alphanumeric()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn links_to_walls_of_a_sweep_along_several_curves_drop_their_saved_names() {
        let mut v = json!({"features": [
            {"id": 8, "type": "sweep", "path": ["l1", "a1"]},
            {"id": 9, "type": "sweep", "path": ["l1"]},
            {"id": 10, "type": "sketch", "sketch": {"links": [
                {"id": "j1", "vertex_name": "F8:side:l1#1|F8:side:l2|F4:top"},
                {"id": "j2", "edge_name": "F33:top#[F8:side:l2]|F33:side:l1"},
                {"id": "j3", "vertex_name": "F9:side:l1|F9:side:l2|F9:start"},
                {"id": "j4", "edge_name": "F18:side:l1|F4:top"},
                {"id": "j5", "auto_face": true, "face_name": "F8:side:l1#1"},
                {"id": "j6", "face_name": "F8:side:l1#2"}
            ]}}
        ]});
        super::step(&mut v).unwrap();
        let links = v.pointer("/features/2/sketch/links").unwrap();
        assert_eq!(links[0].get("vertex_name"), None);
        assert_eq!(links[1].get("edge_name"), None, "a name built beside a wall of the sweep");
        assert!(links[0]["name_dropped"] == json!(true) && links[1]["name_dropped"] == json!(true));
        assert_eq!(links[2]["vertex_name"], json!("F9:side:l1|F9:side:l2|F9:start"), "a sweep along one curve");
        assert_eq!(links[3]["edge_name"], json!("F18:side:l1|F4:top"), "F18 is not F8");
        assert_eq!(links[4]["face_name"], json!("F8:side:l1#1"), "a sketch plane's own face");
        assert_eq!(links[5].get("face_name"), None);
        assert!([2, 3, 4, 5].iter().all(|i| links[*i].get("name_dropped").is_none()));
        let mut empty = json!({"name": "Untitled"});
        super::step(&mut empty).unwrap();
        assert_eq!(empty, json!({"name": "Untitled"}));
    }
}
