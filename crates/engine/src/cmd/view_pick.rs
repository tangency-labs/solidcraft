//! `view.pick` with `rays` in any session, headless too (#16240): where each world ray first
//! meets the design's shown bodies, decided on their exact faces, edges and vertices
//! (`Body::fire_ray`), never on a display mesh. The running app answers `view.pick` itself
//! (its pixels need its camera and pick index) and never reaches this command.
use std::collections::{BTreeMap, VecDeque};

use serde_json::{Value, json};
use solidcore_record::Vec3;

use super::CommandSpec;
use crate::params::bad;
use crate::schema::view_pick::{ViewPickParams, ViewPickResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const PICK_CONTRACT: TypedContract<ViewPickParams> = TypedContract::new();

/// At most this many rays a call, as the app allows pixels and rays together.
const MAX_RAYS: usize = 1024;

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("view.pick", "Pick by Ray", crate::checked!("view.pick", pick))
    .icon("inspect")
    .noundo()
    .read_only()
    .contract(&PICK_CONTRACT)
    .output(output_schema_for::<ViewPickResult>)
    .params(
        "rays: [{origin: [x,y,z], dir: [x,y,z]}] (at most 1024); all?: bool (every body and point cloud each ray meets, nearest first); \
         clouds?: bool (pick visible point-cloud points too; default: only when cloud_radius or point_px is given), \
         cloud_radius?: mm (default 1.0, 0 disables point-cloud picks on rays) → frame: \"rays\", \
         hits: one per ray, null where it meets nothing, else for a body {pixel (the ray, echoed as the app echoes it), body, face, point, normal, depth} with edge or vertex (record index) where \
         the ray passes through one and occurrence, occurrence_name, component for a placed part's body, or for a point cloud {pixel, body: null, face: null, cloud, cloud_name, point_index, point, depth, distance_mm}; decided on exact geometry, the same \
         answer headless as with the app. pixels, frame, width, height, edges, edge_px, point_px, include_ghosted: answered only by the running app",
    )];

fn pick(s: &mut Session, p: &Value) -> Result<Value> {
    const ID: &str = "view.pick";
    if p.get("pixels").is_some() {
        return Err(bad(ID, "`pixels` need the running SolidCraft app (its camera and window); give world `rays` [{origin, dir}] here"));
    }
    let rays = p.get("rays").and_then(Value::as_array).ok_or_else(|| bad(ID, "give `rays`: [{origin: [x, y, z], dir: [x, y, z]}]"))?;
    if rays.is_empty() || rays.len() > MAX_RAYS {
        return Err(bad(ID, format!("give 1 to {MAX_RAYS} rays")));
    }
    let all = p.get("all").and_then(Value::as_bool).unwrap_or(false);
    let clouds_on = crate::point_cloud_pick::clouds_requested(p).map_err(|e| bad(ID, e))?;
    let cloud_radius = match p.get("cloud_radius") {
        None | Some(Value::Null) => crate::point_cloud_pick::DEFAULT_CLOUD_RADIUS_MM,
        Some(v) => v
            .as_f64()
            .filter(|x| x.is_finite() && (0.0..=crate::point_cloud_pick::MAX_CLOUD_RADIUS_MM).contains(x))
            .ok_or_else(|| bad(ID, format!("`cloud_radius` must be a number from 0 to {}", crate::point_cloud_pick::MAX_CLOUD_RADIUS_MM)))?,
    };
    let mut inputs = Vec::with_capacity(rays.len());
    for r in rays {
        let triple = |key: &str| -> Result<Vec3> {
            let v = r.get(key).and_then(Value::as_array).ok_or_else(|| bad(ID, format!("a ray needs `{key}` [x, y, z]")))?;
            let xyz: Vec<f64> = v.iter().filter_map(Value::as_f64).filter(|x| x.is_finite()).collect();
            match xyz.as_slice() {
                [x, y, z] if v.len() == 3 => Ok(Vec3::new(*x, *y, *z)),
                _ => Err(bad(ID, format!("`{key}` must be [x, y, z] finite numbers"))),
            }
        };
        let (o, d) = (triple("origin")?, triple("dir")?);
        if d.x == 0.0 && d.y == 0.0 && d.z == 0.0 {
            return Err(bad(ID, "a ray needs a non-zero `dir`"));
        }
        inputs.push((r, o, d));
    }

    let st = s.world_state()?;
    let occ = occurrence_map(s);
    let comps = component_names(s);
    let names: BTreeMap<u64, &str> = s.doc.components.iter().map(|c| (c.id, c.name.as_str())).collect();
    let shown: Vec<_> = st.bodies.iter().filter(|b| s.body_visible(&b.name) && s.display_opacity(&b.name) > 0.0).collect();
    let clouds: Vec<_> = if clouds_on && cloud_radius > 0.0 { crate::view::shown_point_clouds(s).collect() } else { Vec::new() };
    let cloud_indexes: Vec<Option<solidcraft_geom::PointCloudIndex>> =
        clouds.iter().map(|c| (inputs.len() > 1 && c.len() > 512).then(|| solidcraft_geom::PointCloudIndex::new(c.points.values()))).collect();
    let mut hits = Vec::with_capacity(inputs.len());
    for (raw, o, d) in inputs {
        let mut found: Vec<Value> = Vec::new();
        let mut depths: Vec<f64> = Vec::new();
        // What hides a cloud point: the nearest body crossing the section keeps. A crossing the
        // section cuts away shows nothing, so it hides nothing.
        let mut cover: Option<f64> = None;
        for b in &shown {
            let hit = b.body.fire_ray(o, d).map_err(|e| bad(ID, format!("body {}: {e}", b.name)))?;
            let Some(h) = hit else { continue };
            let mut j = json!({
                "pixel": raw,
                "body": b.name,
                "face": h.face,
                "point": [h.point.x, h.point.y, h.point.z],
                "normal": [h.normal.x, h.normal.y, h.normal.z],
                "depth": h.depth,
            });
            if let Some(e) = h.edge {
                j["edge"] = json!({"index": e});
            }
            if let Some(v) = h.vertex {
                j["vertex"] = json!({"index": v});
            }
            // A placed body's component is the one its placement places (every placement's world
            // name differs); a body that is not placed is named by the component that owns it.
            let mut component = comps.get(&b.name).map(String::as_str);
            if let Some((path, name, id)) = occ.get(&b.name) {
                j["occurrence"] = json!(path);
                j["occurrence_name"] = json!(name);
                component = names.get(id).copied().or(component);
            }
            if let Some(name) = component {
                j["component"] = json!(name);
            }
            if !clouds.is_empty() {
                let shown_from = if s.section.is_empty() {
                    Some(h.depth)
                } else {
                    kept_material_from(&b.body, o, d, h.depth, &s.section).map_err(|e| bad(ID, format!("body {}: {e}", b.name)))?
                };
                if let Some(t) = shown_from {
                    cover = Some(cover.map_or(t, |c: f64| c.min(t)));
                }
            }
            let at = depths.partition_point(|x| *x <= h.depth);
            depths.insert(at, h.depth);
            found.insert(at, j);
        }
        let max_depth = if all { None } else { cover };
        // The nearest cloud point no kept body stands in front of.
        let mut front_cloud: Option<(f64, Value)> = None;
        for (c, index) in clouds.iter().zip(&cloud_indexes) {
            let Some(h) = crate::point_cloud_pick::pick_cloud_ray(c, index.as_ref(), o, d, cloud_radius, 0.0, max_depth, &s.section) else {
                continue;
            };
            let j = json!({
                "pixel": raw,
                "body": null,
                "face": null,
                "cloud": h.cloud,
                "cloud_name": h.cloud_name,
                "point_index": h.index,
                "point": [h.point.x, h.point.y, h.point.z],
                "depth": h.depth,
                "distance_mm": h.ray_dist,
            });
            if !all {
                // A body at the same depth keeps the pick, as before clouds could be picked.
                if cover.is_none_or(|c| h.depth < c) && front_cloud.as_ref().is_none_or(|(t, _)| h.depth < *t) {
                    front_cloud = Some((h.depth, j));
                }
                continue;
            }
            let at = depths.partition_point(|x| *x <= h.depth);
            depths.insert(at, h.depth);
            found.insert(at, j);
        }
        hits.push(match (all, front_cloud) {
            (true, _) => Value::Array(found),
            // In front of every kept body: a body the section cuts away there shows nothing.
            (false, Some((_, cloud))) => cloud,
            (false, None) => found.into_iter().next().unwrap_or(Value::Null),
        });
    }
    Ok(json!({"frame": "rays", "hits": hits}))
}

/// How far past a crossing the next ray starts, so it does not meet the same face again.
const CROSSING_STEP: f64 = 1e-6;

/// Where along the ray `(o, d)` (unit `d`) the body first shows under a Section Analysis: the
/// first surface crossing the section keeps, or a section plane crossed inside the material (its
/// cut face), whichever is nearer. `None` where the section removes every bit of the body the ray
/// passes through. `first` is the depth (distance) of the ray's first crossing of the body.
fn kept_material_from(
    body: &solidcraft_kernel::Body,
    o: Vec3,
    d: Vec3,
    first: f64,
    section: &[crate::SectionPlane],
) -> std::result::Result<Option<f64>, String> {
    use solidcraft_geom::prelude::*;
    let Some(d) = d.normalized() else { return Ok(Some(first)) };
    // Clip the forward ray `t >= 0` against every section half-space `n · (p - origin) <= 0` to
    // obtain the retained interval `[t_lo, t_hi]`. Jumping straight to `t_lo` avoids stepping
    // through arbitrarily many cut-away crossings on comb-shaped bodies.
    let mut t_lo = 0.0f64;
    let mut t_hi = f64::INFINITY;
    for plane in section {
        let along = plane.normal.dot(d);
        let rhs = plane.normal.dot(plane.origin - o);
        if along.abs() < 1e-12 {
            if rhs < -1e-6 {
                return Ok(None);
            }
        } else if along > 0.0 {
            t_hi = t_hi.min(rhs / along);
        } else {
            t_lo = t_lo.max(rhs / along);
        }
    }
    if !t_lo.is_finite() || t_lo > t_hi + 1e-6 {
        return Ok(None);
    }

    let keeps = |p: Vec3| crate::point_cloud_pick::section_keeps(section, p);
    // If the ray enters the kept interval through a section plane (`t_lo > 0`) inside the body,
    // that cut face is the nearest kept material — including when the ray starts inside the
    // removed half of the body (`t_lo < first`).
    if t_lo > 0.0 {
        let p = o + d * t_lo;
        let aabb = body.outline_bounds();
        let pad = 1e-3;
        let in_box = !aabb.is_empty()
            && p.x >= aabb.min.x - pad
            && p.x <= aabb.max.x + pad
            && p.y >= aabb.min.y - pad
            && p.y <= aabb.max.y + pad
            && p.z >= aabb.min.z - pad
            && p.z <= aabb.max.z + pad;
        if in_box && keeps(p) && matches!(body.classify_point(p), Ok(solidcraft_kernel::PointClass::Inside)) {
            return Ok(Some(t_lo));
        }
    }

    // Otherwise find the first kept surface crossing in `[t_lo, t_hi]`.
    if keeps(o + d * first) {
        return Ok(Some(first));
    }
    if first > t_hi + 1e-6 {
        return Ok(None);
    }
    let mut start = (t_lo - 1e-7).max(first + CROSSING_STEP);
    for _ in 0..2 {
        let Some(h) = body.fire_ray(o + d * start, d).map_err(|e| e.to_string())? else {
            return Ok(None);
        };
        let hit_t = start + h.depth;
        if keeps(o + d * hit_t) {
            return Ok(Some(hit_t));
        }
        if hit_t > t_hi + 1e-6 {
            return Ok(None);
        }
        start = hit_t.max(t_lo) + CROSSING_STEP;
    }
    Ok(None)
}

/// Body world name → its outermost placement (path, name) and the component it places,
/// shallower placements first, bounded as the app's pick bounds it.
fn occurrence_map(s: &Session) -> BTreeMap<String, (Vec<u64>, String, u64)> {
    let mut out = BTreeMap::new();
    let mut queue: VecDeque<Vec<u64>> = VecDeque::from([vec![]]);
    let mut budget = 10_000usize;
    while let Some(path) = queue.pop_front() {
        let Ok(children) = s.occurrence_children(&path) else { continue };
        for placement in children {
            if budget == 0 {
                return out;
            }
            budget -= 1;
            for b in &placement.bodies {
                out.entry(b.world.clone()).or_insert_with(|| (placement.path.clone(), placement.name.clone(), placement.component));
            }
            queue.push_back(placement.path.clone());
        }
    }
    out
}

/// Body name → the name of the component that owns it (the root component names nothing).
fn component_names(s: &Session) -> BTreeMap<String, String> {
    let names: BTreeMap<u64, &str> = s.doc.components.iter().map(|c| (c.id, c.name.as_str())).collect();
    s.doc.body_components.iter().filter_map(|(body, id)| names.get(id).map(|n| (body.clone(), (*n).to_string()))).collect()
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::Session;

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        match s.execute(id, &p) {
            Ok(v) => v,
            Err(e) => panic!("{id} {p}: {e}"),
        }
    }

    fn xyz(v: &Value) -> [f64; 3] {
        let f = |i: usize| v.get(i).and_then(Value::as_f64).unwrap_or(f64::NAN);
        [f(0), f(1), f(2)]
    }

    /// The bar of #16240: radius 20 along X. A ray straight down at x = 0, y = 10 meets it at
    /// z = √(20² − 10²) on the exact cylinder, its normal (0, 10, √300)/20; the depth is from
    /// z = 50. A ray beside the bar meets nothing, and `all` lists every body a ray crosses,
    /// nearest first.
    #[test]
    fn a_ray_meets_the_exact_surface_headless() {
        let mut s = Session::default();
        let bar = json!({"radius": 20, "height": 60, "base": [-30, 0, 0], "axis": "X", "name": "Bar", "body_name": "Bar"});
        run(&mut s, "solid.cylinder", bar);
        let r = run(&mut s, "view.pick", json!({"rays": [{"origin": [0, 10, 50], "dir": [0, 0, -1]}, {"origin": [0, 30, 50], "dir": [0, 0, -1]}]}));
        assert_eq!(r["frame"], "rays");
        let hit = &r["hits"][0];
        let z = 300f64.sqrt();
        let [px, py, pz] = xyz(&hit["point"]);
        assert!(px.abs() < 1e-9 && (py - 10.0).abs() < 1e-9 && (pz - z).abs() < 1e-9, "{hit}");
        let [nx, ny, nz] = xyz(&hit["normal"]);
        assert!(nx.abs() < 1e-9 && (ny - 0.5).abs() < 1e-9 && (nz - z / 20.0).abs() < 1e-9, "{hit}");
        assert!((hit["depth"].as_f64().unwrap_or(f64::NAN) - (50.0 - z)).abs() < 1e-9, "{hit}");
        assert_eq!(hit["body"], "Bar");
        assert!(hit["face"].is_u64(), "{hit}");
        assert_eq!(r["hits"][1], Value::Null);

        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [-5, 5, -60], "name": "Below"}));
        let r = run(&mut s, "view.pick", json!({"rays": [{"origin": [0, 10, 50], "dir": [0, 0, -2]}], "all": true}));
        let list = r["hits"][0].as_array().cloned().unwrap_or_default();
        let depths: Vec<f64> = list.iter().filter_map(|h| h["depth"].as_f64()).collect();
        assert_eq!(list.len(), 2, "{r}");
        assert!((depths[0] - (50.0 - z)).abs() < 1e-9 && (depths[1] - 100.0).abs() < 1e-9, "{r}");
    }

    /// Every placement of a part names the part's component, also the copies whose bodies carry
    /// their placement in their world name.
    #[test]
    fn every_placement_of_a_part_names_its_component() {
        let mut s = Session::default();
        run(&mut s, "component.create", json!({"name": "Plate"}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 2, "corner": [0, 0, 0]}));
        run(&mut s, "component.activate", json!({"component": 0}));
        run(&mut s, "occurrence.copy", json!({"component": "Plate", "translate": [50, 0, 0]}));
        let rays: Vec<Value> = [5, 55].iter().map(|x| json!({"origin": [x, 5, 20], "dir": [0, 0, -1]})).collect();
        let r = run(&mut s, "view.pick", json!({ "rays": rays }));
        let hits = r["hits"].as_array().cloned().unwrap_or_default();
        assert_eq!(hits.len(), 2, "{r}");
        assert_ne!(hits[0]["body"], hits[1]["body"], "two placements, two world bodies: {r}");
        assert_ne!(hits[0]["occurrence"], hits[1]["occurrence"], "{r}");
        for h in &hits {
            assert_eq!(h["component"], "Plate", "{h}");
        }
    }

    /// Pixels need the running app's view, and a ray must be finite with a direction.
    #[test]
    fn pixels_and_bad_rays_are_refused_by_name() {
        let mut s = Session::default();
        let refused = |s: &mut Session, p: Value| s.execute("view.pick", &p).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(refused(&mut s, json!({"pixels": [[1, 1]]})).contains("running SolidCraft app"));
        assert!(refused(&mut s, json!({})).contains("rays"));
        assert!(refused(&mut s, json!({"rays": [{"origin": [0, 0, 0], "dir": [0, 0, 0]}]})).contains("non-zero"));
        assert!(refused(&mut s, json!({"rays": [{"origin": [0, 0], "dir": [0, 0, 1]}]})).contains("origin"));
        assert!(refused(&mut s, json!({"rays": []})).contains("1 to 1024"));
        assert!(refused(&mut s, json!({"rays": [{"origin": [0, 0, 0], "dir": [0, 0, 1]}], "cloud_radius": -1})).contains("cloud_radius"));
        assert!(refused(&mut s, json!({"rays": [{"origin": [0, 0, 0], "dir": [0, 0, 1]}], "clouds": "yes"})).contains("clouds"));
    }

    /// Headless `view.pick` picks visible point-cloud points in world space (origin + transform),
    /// respects front-cluster depth ranking, filters, visibility, section clipping, body
    /// occlusion, `cloud_radius`, and `all` (#8933).
    #[test]
    fn a_ray_picks_visible_point_cloud_points_and_respects_filters_sections_and_occlusion() {
        use solidcore_record::Vec3;
        use solidcraft_doc::point_cloud::{ColorBy, Colors, Column, Complex, Filter, Floats, Keep, On, PointCloud, Points};
        let mut s = Session::default();
        // Cloud with origin (100, 0, 0) and a +90° rotation about Z + translation (0, 10, 0):
        // local (0, 0, 20) -> origin+(0,0,20) = (100, 0, 20) -> world (0, 110, 20).
        // local (0, 0, 5)  -> origin+(0,0,5)  = (100, 0, 5)  -> world (0, 110, 5) (back wall).
        // local (0, 0.2, 20) -> origin+(0,0.2,20) = (100, 0.2, 20) -> world (-0.2, 110, 20).
        let rot_z90 = [[0.0, 1.0, 0.0, 0.0], [-1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 10.0, 0.0, 1.0]];
        let cloud = PointCloud {
            id: 1,
            name: "Scan".into(),
            file: "scan.ply".into(),
            origin: Vec3::new(100.0, 0.0, 0.0),
            points: Points::new(vec![0.0, 0.0, 5.0, 0.0, 0.0, 20.0, 0.0, 0.2, 20.0].into()),
            colors: Colors::default(),
            source_points: 3,
            units: "mm".into(),
            transform: rot_z90,
            point_size: 2.0,
            color_by: ColorBy::File,
            color: None,
            height_range: None,
            visible: true,
            complex: Complex {
                columns: vec![Column { name: "intensity".into(), on: On::Point, integer: false, values: Floats(vec![1.0, 0.5, 0.9].into()) }],
                ..Default::default()
            },
            color_property: None,
            filters: Vec::new(),
            source: String::new(),
            max_points: None,
        };
        s.doc_mut().point_clouds.push(cloud);

        // Ray straight down through (0, 110, 50): front point (index 1 at z=20) beats back wall (index 0 at z=5)
        // and off-axis point (index 2 at x=-0.2).
        let ray = json!({"origin": [0, 110, 50], "dir": [0, 0, -1]});
        let r = run(&mut s, "view.pick", json!({"rays": [ray], "clouds": true}));
        let hit = &r["hits"][0];
        assert_eq!(hit["cloud"], 1, "{hit}");
        assert_eq!(hit["cloud_name"], "Scan", "{hit}");
        assert_eq!(hit["point_index"], 1, "{hit}");
        let [px, py, pz] = xyz(&hit["point"]);
        assert!(px.abs() < 1e-9 && (py - 110.0).abs() < 1e-9 && (pz - 20.0).abs() < 1e-9, "{hit}");
        assert!((hit["depth"].as_f64().unwrap_or(f64::NAN) - 30.0).abs() < 1e-9, "{hit}");

        // A call that does not ask for clouds keeps its earlier answer: the ray meets nothing.
        let r = run(&mut s, "view.pick", json!({"rays": [ray]}));
        assert_eq!(r["hits"][0], Value::Null, "{r}");

        // cloud_radius: 0 disables cloud ray hits.
        let r = run(&mut s, "view.pick", json!({"rays": [ray], "cloud_radius": 0}));
        assert_eq!(r["hits"][0], Value::Null);

        // Active filter removes index 1 (intensity 0.5 < 0.7), so index 2 (intensity 0.9, z=20) wins.
        s.doc_mut().point_clouds[0].filters.push(Filter { property: "intensity".into(), keep: Keep::Range { range: [0.7, 1.0] } });
        let r = run(&mut s, "view.pick", json!({"rays": [ray], "clouds": true}));
        assert_eq!(r["hits"][0]["point_index"], 2, "{r}");

        // Section plane at z=15 removes z>15 (indices 1 & 2), leaving index 0 (z=5, intensity 1.0).
        run(&mut s, "inspect.section", json!({"plane": {"origin": [0, 0, 15], "normal": [0, 0, 1]}}));
        let r = run(&mut s, "view.pick", json!({"rays": [ray], "clouds": true}));
        assert_eq!(r["hits"][0]["point_index"], 0, "{r}");
        run(&mut s, "inspect.section", json!({"clear": true}));

        // A solid box in front (z=25..35) occludes the cloud, while `all: true` returns both nearest first.
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [-5, 105, 25], "body_name": "Shield"}));
        let r = run(&mut s, "view.pick", json!({"rays": [ray], "clouds": true}));
        assert_eq!(r["hits"][0]["body"], "Shield", "{r}");
        let r = run(&mut s, "view.pick", json!({"rays": [ray], "all": true, "clouds": true}));
        let list = r["hits"][0].as_array().cloned().unwrap_or_default();
        assert_eq!(list.len(), 2, "{r}");
        assert_eq!(list[0]["body"], "Shield", "{r}");
        assert_eq!(list[1]["cloud"], 1, "{r}");

        // Without `clouds` the box answers as it always has, and `all` lists only the box.
        let r = run(&mut s, "view.pick", json!({"rays": [ray], "all": true}));
        assert_eq!(r["hits"][0].as_array().map(Vec::len), Some(1), "{r}");

        // A section keeping z <= 22 cuts the whole box away: it no longer hides the cloud, and
        // the scan point the filter keeps at z = 20 (index 2) answers instead of the box the
        // section removed.
        run(&mut s, "inspect.section", json!({"plane": {"origin": [0, 0, 22], "normal": [0, 0, 1]}}));
        let r = run(&mut s, "view.pick", json!({"rays": [ray], "clouds": true}));
        assert_eq!(r["hits"][0]["cloud"], 1, "{r}");
        assert_eq!(r["hits"][0]["point_index"], 2, "{r}");
        run(&mut s, "inspect.section", json!({"clear": true}));

        // A section at z = 30 cuts away only the shield's top half: its cut face (z = 30) and
        // the half left (z = 25..30) still stand in front of the scan point at z = 20.
        run(&mut s, "inspect.section", json!({"plane": {"origin": [0, 0, 30], "normal": [0, 0, 1]}}));
        let r = run(&mut s, "view.pick", json!({"rays": [ray], "clouds": true}));
        assert_eq!(r["hits"][0]["body"], "Shield", "{r}");
        // The same with a ray that is not of unit length.
        let long = json!({"origin": [0, 110, 50], "dir": [0, 0, -7]});
        let r = run(&mut s, "view.pick", json!({"rays": [long], "clouds": true}));
        assert_eq!(r["hits"][0]["body"], "Shield", "{r}");
        // Ray starting inside the cut-away top half (z = 32) looking down into the kept half
        // (z = 25..30): the cut face at z = 30 (depth 2) is nearer than the far wall at z = 25
        // (depth 7) and still hides a scan point embedded at z = 28 (depth 4).
        {
            let mut embedded = s.doc.point_clouds[0].clone();
            embedded.id = 2;
            embedded.name = "Embedded".into();
            embedded.origin = Vec3::ZERO;
            embedded.transform = solidcraft_doc::IDENTITY;
            embedded.points = Points::new(vec![0.0, 110.0, 28.0].into());
            embedded.filters.clear();
            embedded.complex = Complex::default();
            s.doc_mut().point_clouds.push(embedded);
            let inside_ray = json!({"origin": [0, 110, 32], "dir": [0, 0, -1]});
            let r = run(&mut s, "view.pick", json!({"rays": [inside_ray], "clouds": true}));
            assert_eq!(r["hits"][0]["body"], "Shield", "{r}");
            s.doc_mut().point_clouds.pop();
        }
        run(&mut s, "inspect.section", json!({"clear": true}));

        // Section entering through an air gap of a single body (two teeth joined on the side:
        // upper tooth z=40..45 removed, air gap z=35..40, lower tooth z=25..35 kept). The section
        // plane at z=37 lies outside material, yet the kept lower tooth still hides the scan at z=20.
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 5, "corner": [-5, 105, 40], "body_name": "UpperTooth"}));
        run(&mut s, "solid.box", json!({"length": 2, "width": 10, "height": 20, "corner": [3, 105, 25], "body_name": "Spine"}));
        run(&mut s, "solid.combine", json!({"target": "Shield", "tools": ["UpperTooth", "Spine"], "operation": "join"}));
        run(&mut s, "inspect.section", json!({"plane": {"origin": [0, 0, 37], "normal": [0, 0, 1]}}));
        let r = run(&mut s, "view.pick", json!({"rays": [ray], "clouds": true}));
        assert_eq!(r["hits"][0]["body"], "Shield", "{r}");
        run(&mut s, "inspect.section", json!({"clear": true}));

        // Hidden cloud is not picked even with `all: true`.
        s.doc_mut().point_clouds[0].visible = false;
        let r = run(&mut s, "view.pick", json!({"rays": [ray], "all": true, "clouds": true}));
        let list = r["hits"][0].as_array().cloned().unwrap_or_default();
        assert_eq!(list.len(), 1, "{r}");
        assert_eq!(list[0]["body"], "Shield", "{r}");

        // Measuring two picked cloud points computes their exact distance.
        s.doc_mut().point_clouds[0].visible = true;
        let items = json!([
            {"type": "point_cloud_point", "cloud": 1, "index": 0, "point": [0.0, 110.0, 5.0]},
            {"type": "point_cloud_point", "cloud": 1, "index": 1, "point": [0.0, 110.0, 20.0]},
        ]);
        let m = run(&mut s, "inspect.measure", json!({"items": items}));
        assert!((m["distance_mm"].as_f64().unwrap_or(f64::NAN) - 15.0).abs() < 1e-9, "{m}");
    }

    /// On a slanted front surface with a back wall behind it, the front-cluster band picks the
    /// front point closest to the ray instead of punching through to the back wall or snapping to
    /// the near edge of the pick cylinder; integer label `Values` filters are respected too.
    #[test]
    fn slanted_front_surface_beats_back_wall_and_label_value_filters_hide_points() {
        use solidcore_record::Vec3;
        use solidcraft_doc::point_cloud::{ColorBy, Colors, Column, Complex, Filter, Floats, Keep, Names, On, PointCloud, Points};
        let mut s = Session::default();
        // Slanted front surface z = 20 + x (45° slope):
        //   idx 0: (0.8, 0, 20.8)  — nearer to eye (depth 29.2), off-axis by 0.8 mm
        //   idx 1: (0.0, 0, 20.0)  — on ray axis (depth 30.0), inside front band (29.2 + 2*1.0 = 31.2)
        // Back wall directly on ray axis:
        //   idx 2: (0.0, 0, 10.0)  — on ray axis (depth 40.0), outside front band
        let cloud = PointCloud {
            id: 2,
            name: "Slant".into(),
            file: "slant.ply".into(),
            origin: Vec3::ZERO,
            points: Points::new(vec![0.8, 0.0, 20.8, 0.0, 0.0, 20.0, 0.0, 0.0, 10.0].into()),
            colors: Colors::default(),
            source_points: 3,
            units: "mm".into(),
            transform: solidcraft_doc::IDENTITY,
            point_size: 2.0,
            color_by: ColorBy::File,
            color: None,
            height_range: None,
            visible: true,
            complex: Complex {
                columns: vec![Column { name: "part".into(), on: On::Point, integer: true, values: Floats(vec![1.0, 1.0, 2.0].into()) }],
                names: vec![Names { property: "part".into(), values: vec![(1, "front".into()), (2, "back".into())] }],
                ..Default::default()
            },
            color_property: None,
            filters: Vec::new(),
            source: String::new(),
            max_points: None,
        };
        s.doc_mut().point_clouds.push(cloud);

        // Ray down along (0, 0, 50) -> (0, 0, -1) with cloud_radius = 1.0 mm:
        // Front on-axis point (idx 1) beats both the nearer off-axis front point (idx 0)
        // and the back wall (idx 2).
        let ray = json!({"origin": [0, 0, 50], "dir": [0, 0, -1]});
        let r = run(&mut s, "view.pick", json!({"rays": [ray], "cloud_radius": 1.0}));
        assert_eq!(r["hits"][0]["point_index"], 1, "{r}");

        // Label `Values` filter keeping only part == 2 hides the front surface, exposing idx 2.
        s.doc_mut().point_clouds[0].filters.push(Filter { property: "part".into(), keep: Keep::Values { values: vec![2] } });
        let r = run(&mut s, "view.pick", json!({"rays": [ray], "cloud_radius": 1.0}));
        assert_eq!(r["hits"][0]["point_index"], 2, "{r}");
    }
}
