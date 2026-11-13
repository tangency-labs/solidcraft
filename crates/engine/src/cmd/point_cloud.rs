//! Point clouds (#5151): scanned points (PLY, XYZ or PTS files) kept in the design as a reference
//! to model from. They are drawn as points and never become bodies.

use serde_json::{Value, json};
use solidcraft_doc::Mat;
use solidcraft_doc::point_cloud::{
    ColorBy, Colors, Complex, Filter, Keep, MAX_COORD, MAX_FILTERS, MAX_NAMES, MAX_POINTS, POINT_SIZE, PointCloud, Points, UNITS, unit_scale,
};

use super::CommandSpec;
use crate::params::{bad, bool_, str_};
use crate::schema::point_cloud::{
    PointCloudDeleteParams, PointCloudDeleteResult, PointCloudFilter, PointCloudFilterKeeps, PointCloudInfo, PointCloudListParams,
    PointCloudListResult, PointCloudProperty, PointCloudReloadParams, PointCloudReloadResult,
};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const RELOAD_CONTRACT: TypedContract<PointCloudReloadParams> = TypedContract::new();
const DELETE_CONTRACT: TypedContract<PointCloudDeleteParams> = TypedContract::new();
const LIST_CONTRACT: TypedContract<PointCloudListParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("pointcloud.insert", "Insert Point Cloud", insert)
        .at("SOLID", "INSERT")
        .icon("import")
        .params("path: .ply, .xyz or .pts file (its points join the design as a point cloud: drawn as points, never a body; a PLY's red, green and blue are its colours, its other properties can colour and filter it; its edge and face elements become segments and triangles, a segment with width and height properties drawn as a bead) → cloud, name, points, segments, triangles, source_points, bounds, warnings; name?; units?: mm|cm|m|in|ft (the file's length unit, default mm); max_points?: most points kept (default 5000000, at most 20000000; a larger file is thinned evenly); center?: bool (move the cloud so the middle of its base sits at the origin); point_size?: pixels (0.5…16, default 2); color_by?: file|single|height or a property's name (the file's colours, the default; one colour; a ramp by height, blue low to red high; or by any property of the file, a ramp over its values or a palette for whole numbers); color?: [r, g, b] 0…1 (the single colour; given alone it sets color_by single, null sets color_by file); color_range?: [low, high] the ramp runs between (design Z in mm by height), or null for the automatic range (by height all but the lowest and highest 2 % of the points; by a property the values it takes over what the filters keep); height_range?: the same as color_range; filters?: [{property, range: [low, high]} or {property, values: [whole numbers]} for a property with named values], or null (only what every filter keeps is shown)"),
    CommandSpec::new("pointcloud.edit", "Edit Point Cloud", edit)
        .icon("edit")
        .params("cloud: id or name; name?; visible?; point_size?: pixels (0.5…16); color_by?: file|single|height or a property's name (the file's colours, the default; one colour; a ramp by height, blue low to red high; or by any property of the file, a ramp over its values or a palette for whole numbers); color?: [r, g, b] 0…1 (the single colour; given alone it sets color_by single, null sets color_by file); color_range?: [low, high] the ramp runs between (design Z in mm by height), or null for the automatic range (by height all but the lowest and highest 2 % of the points; by a property the values it takes over what the filters keep); height_range?: the same as color_range; units?: mm|cm|m|in|ft (the file's length unit: the points are scaled from the unit they came in as); matrix?: [12 or 16 numbers] (where the cloud sits: a rigid row-major 3x4 or 4x4 matrix, as occurrence.move takes it); tolerance?; filters?: [{property, range: [low, high]} or {property, values: [whole numbers]} for a property with named values], or null (only what every filter keeps is shown) → cloud"),
    CommandSpec::new("pointcloud.reload", "Reload Point Cloud", crate::checked!("pointcloud.reload", reload))
        .icon("import")
        .contract(&RELOAD_CONTRACT)
        .output(output_schema_for::<PointCloudReloadResult>)
        .params("cloud: id or name (its file is read again from where it was inserted; name, placement, units, colouring and the filters on properties it still has are kept; read under the limit it was inserted with, so a file that grew is thinned only past that limit) → cloud, points, segments, triangles, warnings"),
    CommandSpec::new("pointcloud.delete", "Delete Point Cloud", crate::checked!("pointcloud.delete", delete))
        .icon("delete")
        .contract(&DELETE_CONTRACT)
        .output(output_schema_for::<PointCloudDeleteResult>)
        .params("cloud: id or name → deleted"),
    CommandSpec::new("pointcloud.list", "Point Clouds", crate::checked!("pointcloud.list", list))
        .icon("list")
        .noundo()
        .doc_only()
        .contract(&LIST_CONTRACT)
        .output(output_schema_for::<PointCloudListResult>)
        .params("→ clouds: [{cloud, name, file, source (the file's path, read again by pointcloud.reload), points, segments, triangles, source_points, units, visible, point_size, color_by (file, single, height or a property's name), color, colored, properties: [{name, integer, span, names}], filters, color_range (the ramp's now, exact over what the filters keep), height_range (the ramp's own range or the automatic one by height), height_auto, bounds: [[x,y,z], [x,y,z]] in the design, matrix}]"),
];

fn units_of(p: &Value, cmd: &str) -> Result<Option<(&'static str, f64)>> {
    match p.get("units") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(u)) => UNITS
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(u))
            .map(|(name, k)| Some((*name, *k)))
            .ok_or_else(|| bad(cmd, format!("`units` must be one of mm, cm, m, in, ft (not `{u}`)"))),
        Some(_) => Err(bad(cmd, "`units` must be one of mm, cm, m, in, ft")),
    }
}

fn point_size(p: &Value, cmd: &str) -> Result<Option<f64>> {
    match p.get("point_size") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|s| s.is_finite() && POINT_SIZE.contains(s))
            .map(Some)
            .ok_or_else(|| bad(cmd, format!("`point_size` must be a number of pixels from {} to {}", POINT_SIZE.start(), POINT_SIZE.end()))),
    }
}

/// `color`: absent (`None`), null for the file's colours (`Some(None)`), or [r, g, b] 0…1.
fn color(p: &Value, cmd: &str) -> Result<Option<Option<[f32; 3]>>> {
    match p.get("color") {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::Array(a)) if a.len() == 3 => {
            let c: Option<Vec<f32>> = a.iter().map(|v| v.as_f64().filter(|x| (0.0..=1.0).contains(x)).map(|x| x as f32)).collect();
            match c.as_deref() {
                Some(&[r, g, b]) => Ok(Some(Some([r, g, b]))),
                _ => Err(bad(cmd, "`color` must be [r, g, b] with each from 0 to 1")),
            }
        }
        Some(_) => Err(bad(cmd, "`color` must be [r, g, b] with each from 0 to 1, or null")),
    }
}

/// `color_by`: absent (`None`), file, single or height, or the name of a property.
fn color_by(p: &Value, cmd: &str) -> Result<Option<(ColorBy, Option<String>)>> {
    match p.get("color_by") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(v)) => Ok(Some(match ColorBy::parse(v) {
            Some(b) if b != ColorBy::Property => (b, None),
            _ => (ColorBy::Property, Some(v.clone())),
        })),
        Some(_) => Err(bad(cmd, "`color_by` must be file, single, height or a property's name")),
    }
}

/// `filters`: absent (`None`), null for none, or [{property, range: [low, high]} or
/// {property, values: [whole numbers]}].
fn filters(p: &Value, cmd: &str) -> Result<Option<Vec<Filter>>> {
    let refuse = || {
        bad(
            cmd,
            format!(
                "`filters` must be at most {MAX_FILTERS} {{property, range: [low, high]}} (lowest first) or {{property, values: [whole numbers]}}, or null"
            ),
        )
    };
    match p.get("filters") {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(Vec::new())),
        Some(Value::Array(a)) if a.len() <= MAX_FILTERS => a
            .iter()
            .map(|f| {
                let property = f.get("property").and_then(Value::as_str).ok_or_else(refuse)?.to_string();
                if let Some(values) = f.get("values") {
                    let values = values.as_array().ok_or_else(refuse)?;
                    let whole = |v: &Value| v.as_i64().and_then(|n| i32::try_from(n).ok());
                    let values = values.iter().map(whole).collect::<Option<Vec<i32>>>().filter(|v| v.len() <= MAX_NAMES).ok_or_else(refuse)?;
                    return Ok(Filter { property, keep: Keep::Values { values } });
                }
                match f.get("range").and_then(Value::as_array).map(|r| r.iter().map(Value::as_f64).collect::<Option<Vec<f64>>>()) {
                    Some(Some(r)) if matches!(r.as_slice(), &[lo, hi] if lo.is_finite() && hi.is_finite() && lo <= hi) => {
                        Ok(Filter { property, keep: Keep::Range { range: [r[0], r[1]] } })
                    }
                    _ => Err(refuse()),
                }
            })
            .collect::<Result<Vec<Filter>>>()
            .map(Some),
        Some(_) => Err(refuse()),
    }
}

/// A cloud colours and filters only by properties it has.
fn check_properties(c: &PointCloud, cmd: &str) -> Result<()> {
    let names: Vec<&str> = c.property_names().collect();
    for n in c.color_property.iter().filter(|_| c.color_by == ColorBy::Property).chain(c.filters.iter().map(|f| &f.property)) {
        if !names.contains(&n.as_str()) {
            return Err(bad(cmd, format!("the point cloud has no property `{n}` (it has {})", names.join(", "))));
        }
    }
    if let Some(f) = c.filters.iter().find(|f| picks_from_no_names(&c.complex, f)) {
        return Err(bad(cmd, format!("`{}` has no named values: filter it with a range", f.property)));
    }
    Ok(())
}

/// A set of values filters a label: a property with named whole numbers. Is this one a set of
/// values on a property with no names?
fn picks_from_no_names(complex: &Complex, f: &Filter) -> bool {
    matches!(f.keep, Keep::Values { .. }) && complex.names(&f.property).is_none()
}

/// `height_range`: absent (`None`), null for the automatic one (`Some(None)`), or [low, high].
fn height_range(p: &Value, cmd: &str) -> Result<Option<Option<[f64; 2]>>> {
    let refuse = || bad(cmd, "`height_range` (or `color_range`) must be [low, high], lowest first, or null");
    match p.get("color_range").or_else(|| p.get("height_range")) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::Array(a)) => match a.iter().map(Value::as_f64).collect::<Option<Vec<f64>>>().as_deref() {
            Some(&[lo, hi]) if lo.is_finite() && hi.is_finite() && lo < hi && lo.abs() <= MAX_COORD && hi.abs() <= MAX_COORD => {
                Ok(Some(Some([lo, hi])))
            }
            _ => Err(refuse()),
        },
        Some(_) => Err(refuse()),
    }
}

/// Apply `color_by`, `color` and `height_range`: a colour given alone means one colour, and
/// `color: null` the file's.
fn set_colors(c: &mut PointCloud, by: Option<(ColorBy, Option<String>)>, color: Option<Option<[f32; 3]>>, range: Option<Option<[f64; 2]>>) {
    if let Some(k) = color {
        c.color = k.or(c.color);
        c.color_by = if k.is_some() { ColorBy::Single } else { ColorBy::File };
    }
    if let Some((b, property)) = by {
        if property != c.color_property && range.is_none() {
            // Another property's values have another range.
            c.height_range = None;
        }
        c.color_by = b;
        c.color_property = property.or(c.color_property.take().filter(|_| b == ColorBy::Property));
    }
    if let Some(r) = range {
        c.height_range = r;
    }
}

/// The cloud's placement as `matrix` takes it: 12 numbers, row by row.
fn matrix_rows(m: &Mat) -> Vec<f64> {
    (0..3).flat_map(|row| (0..4).map(move |col| m[col][row])).collect()
}

fn bounds(c: &PointCloud) -> Option<[[f64; 3]; 2]> {
    let corners = c.world_corners()?;
    // The box around the corners in the design.
    let mut a = [f64::INFINITY; 3];
    let mut b = [f64::NEG_INFINITY; 3];
    for w in corners {
        for (k, v) in [w.x, w.y, w.z].into_iter().enumerate() {
            a[k] = a[k].min(v);
            b[k] = b[k].max(v);
        }
    }
    Some([a, b])
}

fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "pointcloud.insert";
    let path = str_(p, "path").filter(|x| !x.trim().is_empty() && x.len() < 4096).ok_or_else(|| bad(cmd, "`path` is required"))?;
    if !solidcraft_io::is_point_cloud_path(path) {
        return Err(bad(cmd, "only PLY, XYZ and PTS files (.ply, .xyz, .pts) can be inserted as point clouds"));
    }
    let (units, scale) = units_of(p, cmd)?.unwrap_or(("mm", 1.0));
    let max_points = match p.get("max_points") {
        None | Some(Value::Null) => solidcraft_io::DEFAULT_MAX_POINTS,
        Some(v) => v
            .as_u64()
            .filter(|n| (1..=MAX_POINTS as u64).contains(n))
            .map(|n| n as usize)
            .ok_or_else(|| bad(cmd, format!("`max_points` must be a whole number from 1 to {MAX_POINTS}")))?,
    };
    let size = point_size(p, cmd)?.unwrap_or(2.0);
    let colors = (color_by(p, cmd)?, color(p, cmd)?, height_range(p, cmd)?);
    let center = bool_(p, "center").unwrap_or(false);
    let bytes = solidcraft_io::vfs::read(path, solidcraft_io::MAX_POINT_CLOUD_BYTES).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    let read = solidcraft_io::read_point_cloud(&bytes, path, scale, max_points).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    let warnings = read.warnings;
    drop(bytes);
    let file = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string());
    let stem = std::path::Path::new(path).file_stem().map(|n| n.to_string_lossy().trim().to_string()).filter(|n| !n.is_empty());
    let taken = |n: &str| s.doc.point_clouds.iter().any(|c| c.name == n);
    let name = str_(p, "name").filter(|n| !n.trim().is_empty()).map(str::to_string).unwrap_or_else(|| {
        let base = stem.unwrap_or_else(|| "Point Cloud".into());
        if !taken(&base) { base } else { (2..=s.doc.point_clouds.len() + 2).map(|k| format!("{base} ({k})")).find(|n| !taken(n)).unwrap_or(base) }
    });
    let mut cloud = PointCloud {
        id: 0,
        name: name.clone(),
        file,
        origin: read.origin,
        points: Points::new(read.points.into()),
        colors: Colors(read.colors.into()),
        source_points: read.source_points,
        units: units.into(),
        transform: solidcraft_doc::IDENTITY,
        point_size: size,
        color_by: ColorBy::File,
        color: None,
        height_range: None,
        visible: true,
        complex: read.complex,
        color_property: None,
        filters: filters(p, cmd)?.unwrap_or_default(),
        source: std::path::absolute(path).map(|a| a.to_string_lossy().to_string()).unwrap_or_else(|_| path.to_string()),
        max_points: Some(max_points as u64),
    };
    set_colors(&mut cloud, colors.0, colors.1, colors.2);
    check_properties(&cloud, cmd)?;
    if center && let Some((lo, hi)) = cloud.local_bounds() {
        cloud.transform[3] = [-(lo.x + hi.x) / 2.0, -(lo.y + hi.y) / 2.0, -lo.z, 1.0];
    }
    cloud.check()?;
    let doc = s.doc_mut();
    let id = doc.allocate_id()?;
    cloud.id = id;
    let out = json!({
        "cloud": id,
        "name": name,
        "points": cloud.len(),
        "segments": cloud.complex.segment_count(),
        "triangles": cloud.complex.triangle_count(),
        "source_points": cloud.source_points,
        "bounds": bounds(&cloud),
        "warnings": warnings,
    });
    doc.point_clouds.push(cloud);
    Ok(out)
}

fn find(s: &Session, p: &Value, cmd: &str) -> Result<usize> {
    let key = match p.get("cloud") {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(x)) => x.clone(),
        _ => return Err(bad(cmd, "`cloud` must be an id or name")),
    };
    s.doc.point_clouds.iter().position(|c| c.id.to_string() == key || c.name == key).ok_or_else(|| bad(cmd, format!("no point cloud `{key}`")))
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "pointcloud.edit";
    let i = find(s, p, cmd)?;
    let size = point_size(p, cmd)?;
    let colors = (color_by(p, cmd)?, color(p, cmd)?, height_range(p, cmd)?);
    let filters = filters(p, cmd)?;
    let units = units_of(p, cmd)?;
    let placement = match p.get("matrix") {
        None | Some(Value::Null) => None,
        Some(m) => Some(super::component::matrix::rigid(m, super::component::matrix::tolerance(p, cmd)?, cmd)?),
    };
    let name = str_(p, "name").map(str::trim).filter(|n| !n.is_empty()).map(str::to_string);
    if let Some(n) = &name
        && s.doc.point_clouds.iter().enumerate().any(|(k, c)| k != i && &c.name == n)
    {
        return Err(bad(cmd, format!("another point cloud is named `{n}`")));
    }
    let mut c = s.doc.point_clouds.get(i).cloned().ok_or_else(|| bad(cmd, "point cloud"))?;
    if let Some((unit, k)) = units {
        // The points came in as `c.units`: scale them to what they are in `unit`.
        let from = unit_scale(&c.units).unwrap_or(1.0);
        let f = k / from;
        if f != 1.0 {
            c.origin = c.origin * f;
            c.points = Points::new(c.points.values().iter().map(|v| (f64::from(*v) * f) as f32).collect());
        }
        c.units = unit.into();
    }
    if let Some(m) = placement {
        c.transform = m;
    }
    if let Some(v) = size {
        c.point_size = v;
    }
    set_colors(&mut c, colors.0, colors.1, colors.2);
    if let Some(f) = filters {
        c.filters = f;
    }
    check_properties(&c, cmd)?;
    if let Some(v) = bool_(p, "visible") {
        c.visible = v;
    }
    if let Some(n) = name {
        c.name = n;
    }
    // A scale that puts a point out of range is refused, leaving the cloud as it was.
    c.check()?;
    let id = c.id;
    let slot = s.doc_mut().point_clouds.get_mut(i).ok_or_else(|| bad(cmd, "point cloud"))?;
    *slot = c;
    Ok(json!({"cloud": id}))
}

/// The most points a reload keeps: the limit the cloud was read under. A read is a function of
/// the file and that limit, so an unchanged file reads as the very same points, and a file that
/// grew is thinned only past the limit. The counts cannot say this: a cloud has fewer points than
/// its file when some could not be read, too, thinned or not.
fn reload_limit(c: &PointCloud) -> usize {
    c.max_points.map_or(solidcraft_io::DEFAULT_MAX_POINTS, |n| usize::try_from(n).unwrap_or(MAX_POINTS))
}

/// Read the cloud's file again where it was read from, keeping its name, placement, units and
/// how it is shown.
/// The path comes from the design, which may be someone else's, so it is a design path: one on
/// another computer is refused from its text until the user allows that server.
fn reload(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "pointcloud.reload";
    let i = find(s, p, cmd)?;
    let mut c = s.doc.point_clouds.get(i).cloned().ok_or_else(|| bad(cmd, "point cloud"))?;
    if c.source.is_empty() {
        return Err(bad(cmd, format!("point cloud `{}` does not remember its file", c.name)));
    }
    let path = c.source.clone();
    let scale = unit_scale(&c.units).unwrap_or(1.0);
    let max_points = reload_limit(&c);
    let bytes = solidcraft_io::vfs::read_design_path(&path, s.path.as_deref(), solidcraft_io::MAX_POINT_CLOUD_BYTES)
        .map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    let read = solidcraft_io::read_point_cloud(&bytes, &path, scale, max_points).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    c.origin = read.origin;
    c.points = Points::new(read.points.into());
    c.colors = Colors(read.colors.into());
    c.source_points = read.source_points;
    c.complex = read.complex;
    // What the file no longer has is no longer coloured or filtered by.
    let names: Vec<String> = c.property_names().map(str::to_string).collect();
    c.filters.retain(|f| names.contains(&f.property) && !picks_from_no_names(&c.complex, f));
    if c.color_by == ColorBy::Property && !c.color_property.as_ref().is_some_and(|n| names.contains(n)) {
        (c.color_by, c.color_property) = (ColorBy::File, None);
    }
    c.check()?;
    let out = PointCloudReloadResult {
        cloud: c.id,
        points: c.len(),
        segments: c.complex.segment_count(),
        triangles: c.complex.triangle_count(),
        warnings: read.warnings,
    };
    let out = serde_json::to_value(out).map_err(|e| EngineError::Other(e.to_string()))?;
    let slot = s.doc_mut().point_clouds.get_mut(i).ok_or_else(|| bad(cmd, "point cloud"))?;
    *slot = c;
    Ok(out)
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let i = find(s, p, "pointcloud.delete")?;
    let c = s.doc_mut().point_clouds.remove(i);
    serde_json::to_value(PointCloudDeleteResult { deleted: c.id, param_warnings: Vec::new() }).map_err(|e| EngineError::Other(e.to_string()))
}

fn list(s: &mut Session, _: &Value) -> Result<Value> {
    let clouds: Vec<PointCloudInfo> = s
        .doc
        .point_clouds
        .iter()
        .map(|c| PointCloudInfo {
            cloud: c.id,
            name: c.name.clone(),
            file: c.file.clone(),
            source: c.source.clone(),
            points: c.len(),
            segments: c.complex.segment_count(),
            triangles: c.complex.triangle_count(),
            source_points: c.source_points,
            units: c.units.clone(),
            visible: c.visible,
            point_size: c.point_size,
            color_by: c.colored_property().filter(|_| c.color_by == ColorBy::Property).unwrap_or(c.color_by.name()).to_string(),
            color: c.color,
            colored: !c.colors.is_empty(),
            properties: c
                .property_names()
                .map(|n| PointCloudProperty {
                    name: n.to_string(),
                    integer: c.complex.integer(n),
                    span: c.span(n),
                    names: c.complex.names(n).map(|m| m.values.clone()),
                })
                .collect(),
            filters: c
                .filters
                .iter()
                .map(|f| PointCloudFilter {
                    property: f.property.clone(),
                    keep: match &f.keep {
                        Keep::Range { range } => PointCloudFilterKeeps::Range { range: *range },
                        Keep::Values { values } => PointCloudFilterKeeps::Values { values: values.clone() },
                    },
                })
                .collect(),
            color_range: solidcraft_doc::point_cloud_distribution::color_range(
                c,
                &solidcraft_doc::point_cloud_distribution::Geometry::of(c),
                &c.filters,
            ),
            height_range: c.height_range_now(),
            height_auto: c.height_range.is_none(),
            bounds: bounds(c),
            matrix: matrix_rows(&c.transform),
        })
        .collect();
    serde_json::to_value(PointCloudListResult { clouds, param_warnings: Vec::new() }).map_err(|e| EngineError::Other(e.to_string()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use solidcraft_doc::point_cloud::ColorBy;

    use crate::Session;

    fn ply(dir: &std::path::Path, name: &str, points: &[[f32; 3]], colors: bool) -> String {
        let mut b = format!(
            "ply\nformat binary_little_endian 1.0\ncomment a test scan\nelement vertex {}\nproperty float x\nproperty float y\nproperty float z\n",
            points.len()
        );
        if colors {
            b.push_str("property uchar red\nproperty uchar green\nproperty uchar blue\n");
        }
        b.push_str("end_header\n");
        let mut b = b.into_bytes();
        for (i, p) in points.iter().enumerate() {
            for v in p {
                b.extend(v.to_le_bytes());
            }
            if colors {
                b.extend([i as u8, 100, 200]);
            }
        }
        let path = dir.join(name);
        std::fs::write(&path, b).unwrap();
        path.to_string_lossy().to_string()
    }

    fn dir(test: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("solidcraft-pointcloud-{test}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn insert_a_scan_in_metres_edit_it_and_save_it_with_the_design() {
        let d = dir("insert");
        let path = ply(&d, "room.ply", &[[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 3.0, 1.5], [10.0, 20.0, 2.5]], true);
        let mut s = Session::default();
        let r = s.execute("pointcloud.insert", &json!({"path": path, "units": "m"})).unwrap();
        assert_eq!((r["name"].as_str(), r["points"].as_u64(), r["source_points"].as_u64()), (Some("room"), Some(4), Some(4)));
        assert_eq!(r["bounds"], json!([[0.0, 0.0, 0.0], [10000.0, 20000.0, 2500.0]]));
        let id = r["cloud"].as_u64().unwrap();
        let c = &s.doc.point_clouds[0];
        assert_eq!(c.colors.get(2), Some([2, 100, 200]));
        assert_eq!(c.world_point(2).map(|p| [p.x, p.y, p.z]), Some([2000.0, 3000.0, 1500.0]));
        // The file was in millimetres after all; then moved 100 mm along X.
        s.execute("pointcloud.edit", &json!({"cloud": "room", "units": "mm", "point_size": 4, "color": [1, 0.5, 0]})).unwrap();
        s.execute("pointcloud.edit", &json!({"cloud": id, "matrix": [1, 0, 0, 100, 0, 1, 0, 0, 0, 0, 1, 0]})).unwrap();
        let c = &s.doc.point_clouds[0];
        assert_eq!(c.world_point(3).map(|p| [p.x, p.y, p.z]), Some([110.0, 20.0, 2.5]));
        assert_eq!((c.point_size, c.color, c.color_by), (4.0, Some([1.0, 0.5, 0.0]), ColorBy::Single));
        // By height, its own range, then the automatic one again; back to the file's colours.
        s.execute("pointcloud.edit", &json!({"cloud": id, "color_by": "height", "height_range": [0, 10]})).unwrap();
        let listed = s.execute("pointcloud.list", &json!({})).unwrap();
        assert_eq!((listed["clouds"][0]["color_by"].as_str(), listed["clouds"][0]["height_range"].clone()), (Some("height"), json!([0.0, 10.0])));
        s.execute("pointcloud.edit", &json!({"cloud": id, "height_range": null})).unwrap();
        let listed = s.execute("pointcloud.list", &json!({})).unwrap();
        assert_eq!(listed["clouds"][0]["height_auto"], true);
        assert_eq!(s.doc.point_clouds[0].color_by, ColorBy::Height);
        s.execute("pointcloud.edit", &json!({"cloud": id, "color": null})).unwrap();
        assert_eq!(s.doc.point_clouds[0].color_by, ColorBy::File);
        // The single colour was kept meanwhile.
        s.execute("pointcloud.edit", &json!({"cloud": id, "color_by": "single"})).unwrap();
        assert_eq!(s.doc.point_clouds[0].color, Some([1.0, 0.5, 0.0]));
        let listed = s.execute("pointcloud.list", &json!({})).unwrap();
        assert_eq!(listed["clouds"][0]["units"], "mm");
        assert_eq!(listed["clouds"][0]["matrix"][3], 100.0);
        // Undo-able; saved and read back with the design file, points and all.
        s.execute("pointcloud.edit", &json!({"cloud": id, "visible": false})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.doc.point_clouds[0].visible);
        let back = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
        assert_eq!(back.point_clouds, s.doc.point_clouds);
        let file = solidcraft_io::write_design_container(&s.doc).unwrap();
        let opened = solidcraft_io::read_design(&file).unwrap();
        assert_eq!(opened.point_clouds, s.doc.point_clouds);
        s.execute("pointcloud.delete", &json!({"cloud": id})).unwrap();
        assert!(s.doc.point_clouds.is_empty());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_saved_scan_with_properties_edges_and_faces_opens_again() {
        // The file's normals and other numbers, an edge and a face (#14572): a design file keeps
        // the points apart from them, so they were checked against no points and it didn't open.
        let d = dir("properties");
        let path = d.join("normals.ply");
        let text = "ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\n\
            property float nx\nproperty float ny\nproperty float nz\nproperty int layer\nelement edge 1\nproperty int vertex1\n\
            property int vertex2\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n\
            0 0 0 0 0 1 0\n1 0 0 0 0 1 1\n0 1 0 0 0 1 1\n0 1\n3 0 1 2\n";
        std::fs::write(&path, text).unwrap();
        let mut s = Session::default();
        s.execute("pointcloud.insert", &json!({"path": path})).unwrap();
        assert_eq!(s.doc.point_clouds[0].complex.columns.len(), 4, "nx, ny, nz and layer");
        let compact = solidcraft_io::write_design_container_with_compression(&s.doc, solidcraft_io::DesignCompression::Compact).unwrap();
        for file in [solidcraft_io::write_design_container(&s.doc).unwrap(), compact] {
            let opened = solidcraft_io::read_design(&file).unwrap();
            assert_eq!(opened.point_clouds, s.doc.point_clouds);
        }
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_large_scan_is_thinned_and_centred() {
        let d = dir("thin");
        let points: Vec<[f32; 3]> = (0..100).map(|i| [i as f32, 1000.0, 50.0 + (i % 2) as f32]).collect();
        let path = ply(&d, "line.ply", &points, false);
        let mut s = Session::default();
        let r = s.execute("pointcloud.insert", &json!({"path": path, "max_points": 10, "center": true, "name": "Line"})).unwrap();
        assert_eq!((r["points"].as_u64(), r["source_points"].as_u64()), (Some(10), Some(100)));
        assert_eq!(r["warnings"].as_array().map(Vec::len), Some(1), "{r}");
        // The middle of its base at the origin.
        assert_eq!(r["bounds"], json!([[-45.0, 0.0, 0.0], [45.0, 0.0, 0.0]]));
        assert!(s.doc.point_clouds[0].colors.is_empty());
        // A second insert of the same file gets a name of its own.
        let r = s.execute("pointcloud.insert", &json!({"path": path})).unwrap();
        assert_eq!(r["name"], "line");
        let r = s.execute("pointcloud.insert", &json!({"path": path})).unwrap();
        assert_eq!(r["name"], "line (2)");
        let _ = std::fs::remove_dir_all(d);
    }

    /// A PLY in text with a `layer` property (or none) on each point.
    fn layered(path: &std::path::Path, points: usize, layer: bool) {
        let mut b = format!("ply\nformat ascii 1.0\nelement vertex {points}\nproperty float x\nproperty float y\nproperty float z\n");
        if layer {
            b.push_str("property int layer\n");
        }
        b.push_str("end_header\n");
        for i in 0..points {
            b.push_str(&if layer { format!("{i} 0 0 {i}\n") } else { format!("{i} 0 0\n") });
        }
        std::fs::write(path, b).unwrap();
    }

    #[test]
    fn a_reload_reads_the_file_again_and_keeps_how_the_cloud_is_shown() {
        let d = dir("reload");
        let path = d.join("scan.ply");
        layered(&path, 3, true);
        let mut s = Session::default();
        let id = s.execute("pointcloud.insert", &json!({"path": path, "units": "m"})).unwrap()["cloud"].as_u64().unwrap();
        s.execute(
            "pointcloud.edit",
            &json!({"cloud": id, "name": "Scan", "point_size": 4, "matrix": [1, 0, 0, 100, 0, 1, 0, 0, 0, 0, 1, 0], "color_by": "layer", "filters": [{"property": "layer", "range": [0, 2]}]}),
        )
        .unwrap();
        let listed = s.execute("pointcloud.list", &json!({})).unwrap();
        assert_eq!(listed["clouds"][0]["source"], json!(std::path::absolute(&path).unwrap()));

        layered(&path, 5, true);
        let r = s.execute("pointcloud.reload", &json!({"cloud": "Scan"})).unwrap();
        assert_eq!((r["cloud"].as_u64(), r["points"].as_u64()), (Some(id), Some(5)));
        let c = &s.doc.point_clouds[0];
        assert_eq!((c.name.as_str(), c.units.as_str(), c.point_size, c.color_by), ("Scan", "m", 4.0, ColorBy::Property));
        assert_eq!(c.color_property.as_deref(), Some("layer"));
        assert_eq!(c.filters.len(), 1);
        // In metres and moved, as before: the last point is at 4 m, plus 100 mm.
        assert_eq!(c.world_point(4).map(|p| p.x), Some(4100.0));
        // One undo step takes it back.
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.doc.point_clouds[0].len(), 3);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.doc.point_clouds[0].len(), 5);

        // What the file no longer has is no longer coloured or filtered by.
        layered(&path, 2, false);
        s.execute("pointcloud.reload", &json!({"cloud": id})).unwrap();
        let c = &s.doc.point_clouds[0];
        assert_eq!((c.len(), c.color_by, c.color_property.as_deref(), c.filters.len()), (2, ColorBy::File, None, 0));

        // A file that is gone is refused, and the cloud is as it was, with no undo step.
        std::fs::remove_file(&path).unwrap();
        let (before, undo) = (s.doc.point_clouds.clone(), s.undo.len());
        assert!(s.execute("pointcloud.reload", &json!({"cloud": id})).is_err());
        assert_eq!((&s.doc.point_clouds, s.undo.len()), (&before, undo));
        let _ = std::fs::remove_dir_all(d);
    }

    /// A set of values filters a label, so it lasts as long as the file names that property's
    /// values: left behind, it would make every later `pointcloud.edit` of the cloud refused.
    #[test]
    fn a_reload_drops_a_value_filter_when_the_file_stops_naming_the_values() {
        let d = dir("reload-names");
        let path = d.join("scan.ply");
        let write = |comment: &str| {
            let head = "ply\nformat ascii 1.0\n";
            let body = "element vertex 2\nproperty float x\nproperty float y\nproperty float z\nproperty int layer\nend_header\n0 0 0 0\n1 0 0 1\n";
            std::fs::write(&path, format!("{head}{comment}{body}")).unwrap();
        };
        write("comment layer: 0 first, 1 second\n");
        let mut s = Session::default();
        let id = s.execute("pointcloud.insert", &json!({"path": path})).unwrap()["cloud"].as_u64().unwrap();
        s.execute("pointcloud.edit", &json!({"cloud": id, "filters": [{"property": "layer", "values": [1]}]})).unwrap();

        // Still named: the filter stays.
        s.execute("pointcloud.reload", &json!({"cloud": id})).unwrap();
        assert_eq!(s.doc.point_clouds[0].filters.len(), 1);

        // The same property with no names: the filter goes, and the cloud can still be edited.
        write("");
        s.execute("pointcloud.reload", &json!({"cloud": id})).unwrap();
        assert!(s.doc.point_clouds[0].filters.is_empty());
        s.execute("pointcloud.edit", &json!({"cloud": id, "point_size": 4})).unwrap();
        let _ = std::fs::remove_dir_all(d);
    }

    /// A design may come from anyone, and on Windows only asking about `\\\\host\\share\\x` connects
    /// to that computer with the user's sign-in: the path is refused from its text, with no file
    /// call, until the server is allowed.
    #[test]
    fn a_reload_reaches_no_network_location_until_its_server_is_allowed() {
        use solidcraft_io::vfs::{allowing_hosts, file_calls};
        let d = dir("reload-network");
        let path = d.join("scan.ply");
        layered(&path, 3, false);
        let mut s = Session::default();
        let id = s.execute("pointcloud.insert", &json!({"path": path})).unwrap()["cloud"].as_u64().unwrap();
        for (crafted, says) in [(r"\\evil.example\share\scan.ply", "evil.example"), (r"\\.\pipe\scan.ply", "device")] {
            s.doc_mut().point_clouds[0].source = crafted.into();
            let (before, undo, calls) = (s.doc.point_clouds.clone(), s.undo.len(), file_calls());
            let e = s.execute("pointcloud.reload", &json!({"cloud": id})).unwrap_err().to_string();
            assert!(e.contains(says), "{e}");
            assert_eq!(file_calls(), calls, "{crafted}: nothing was asked of the disk or the network");
            assert_eq!((&s.doc.point_clouds, s.undo.len()), (&before, undo));
        }
        // Allowed, the read goes ahead (and fails here only because no such server answers).
        s.doc_mut().point_clouds[0].source = r"\\evil.example\share\scan.ply".into();
        let calls = file_calls();
        let allowed = allowing_hosts(&["evil.example".to_string()], || s.execute("pointcloud.reload", &json!({"cloud": id})));
        assert!(allowed.is_err());
        assert!(file_calls() > calls, "an allowed server is read");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_reload_reads_a_grown_file_whole_and_keeps_a_thinned_cloud_thinned() {
        let d = dir("reload-limit");
        let (whole, thin) = (d.join("whole.ply"), d.join("thin.ply"));
        layered(&whole, 4, false);
        layered(&thin, 100, false);
        let mut s = Session::default();
        s.execute("pointcloud.insert", &json!({"path": whole, "name": "Whole"})).unwrap();
        // One point in ten is the fewest strides that fit in 11: ten points, under the limit.
        s.execute("pointcloud.insert", &json!({"path": thin, "name": "Thin", "max_points": 11})).unwrap();
        let thinned = s.doc.point_clouds[1].clone();
        assert_eq!((thinned.len(), thinned.source_points), (10, 100));

        layered(&whole, 100, false);
        let r = s.execute("pointcloud.reload", &json!({"cloud": "Whole"})).unwrap();
        assert_eq!(r["points"].as_u64(), Some(100), "a file that grew is not cut to the count it had");

        // The same file reads as the very same points.
        s.execute("pointcloud.reload", &json!({"cloud": "Thin"})).unwrap();
        assert_eq!(s.doc.point_clouds[1].points, thinned.points);
        // A grown one, thinned under the same limit.
        layered(&thin, 500, false);
        let r = s.execute("pointcloud.reload", &json!({"cloud": "Thin"})).unwrap();
        assert_eq!(r["points"].as_u64(), Some(11));
        assert_eq!(s.doc.point_clouds[1].source_points, 500);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_reload_takes_no_unreadable_point_for_thinning() {
        let d = dir("reload-unreadable");
        // `count` points in a line, with the one at `far` out of range, so it cannot be read.
        let line = |count: usize, far: usize| -> Vec<[f32; 3]> { (0..count).map(|i| [if i == far { 1e30 } else { i as f32 }, 0.0, 0.0]).collect() };
        let whole = ply(&d, "whole.ply", &line(4, 1), false);
        let thin = ply(&d, "thin.ply", &line(100, 10), false);
        let mut s = Session::default();
        s.execute("pointcloud.insert", &json!({"path": whole, "name": "Whole"})).unwrap();
        s.execute("pointcloud.insert", &json!({"path": thin, "name": "Thin", "max_points": 10})).unwrap();
        let (kept_whole, kept_thin) = (s.doc.point_clouds[0].clone(), s.doc.point_clouds[1].clone());
        assert_eq!((kept_whole.len(), kept_whole.source_points), (3, 4));
        assert_eq!((kept_thin.len(), kept_thin.source_points), (9, 100));

        // The same files read as the very same points.
        s.execute("pointcloud.reload", &json!({"cloud": "Whole"})).unwrap();
        s.execute("pointcloud.reload", &json!({"cloud": "Thin"})).unwrap();
        assert_eq!(s.doc.point_clouds[0].points, kept_whole.points);
        assert_eq!(s.doc.point_clouds[1].points, kept_thin.points);

        // A file that grew comes in whole, not cut to the count it had.
        ply(&d, "whole.ply", &line(100, 1), false);
        let r = s.execute("pointcloud.reload", &json!({"cloud": "Whole"})).unwrap();
        assert_eq!(r["points"].as_u64(), Some(99));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn hostile_point_cloud_params_never_panic() {
        let d = dir("hostile");
        let path = ply(&d, "p.ply", &[[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]], false);
        let bad = d.join("bad.ply");
        std::fs::write(
            &bad,
            b"ply\nformat binary_little_endian 1.0\nelement vertex 99999999999\nproperty float x\nproperty float y\nproperty float z\nend_header\n",
        )
        .unwrap();
        let mut s = Session::default();
        for p in [
            json!({}),
            json!({"path": ""}),
            json!({"path": d.join("missing.ply").to_string_lossy()}),
            json!({"path": bad.to_string_lossy()}),
            json!({"path": d.join("a.stl").to_string_lossy()}),
            json!({"path": path, "units": "parsec"}),
            json!({"path": path, "units": 3}),
            json!({"path": path, "max_points": 0}),
            json!({"path": path, "max_points": -5}),
            json!({"path": path, "max_points": 1e30}),
            json!({"path": path, "point_size": 1e9}),
            json!({"path": path, "point_size": "big"}),
            json!({"path": path, "color": [2, 0, 0]}),
            json!({"path": path, "color": "red"}),
            json!({"path": path, "color_by": "rainbow"}),
            json!({"path": path, "height_range": [5, 1]}),
            json!({"path": path, "height_range": [0, "x"]}),
            json!({"path": path, "height_range": [0, 1e12]}),
        ] {
            assert!(s.execute("pointcloud.insert", &p).is_err(), "{p}");
        }
        assert!(s.doc.point_clouds.is_empty());
        let id = s.execute("pointcloud.insert", &json!({"path": path})).unwrap()["cloud"].as_u64().unwrap();
        for p in [
            json!({}),
            json!({"cloud": 999}),
            json!({"cloud": [1]}),
            json!({"cloud": id, "matrix": [2, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0]}),
            json!({"cloud": id, "matrix": "x"}),
            json!({"cloud": id, "units": "km"}),
            json!({"cloud": id, "point_size": -1}),
        ] {
            assert!(s.execute("pointcloud.edit", &p).is_err(), "{p}");
        }
        assert!(s.execute("pointcloud.delete", &json!({"cloud": "nope"})).is_err());
        for p in [json!({}), json!({"cloud": 999}), json!({"cloud": [1]})] {
            assert!(s.execute("pointcloud.reload", &p).is_err(), "{p}");
        }
        // A cloud from a design saved before it remembered its file, or naming a file not a scan.
        let source = s.doc.point_clouds[0].source.clone();
        for hostile in ["", bad.to_str().unwrap(), d.to_str().unwrap()] {
            s.doc_mut().point_clouds[0].source = hostile.into();
            let before = s.doc.point_clouds.clone();
            assert!(s.execute("pointcloud.reload", &json!({"cloud": id})).is_err(), "{hostile}");
            assert_eq!(s.doc.point_clouds, before);
        }
        s.doc_mut().point_clouds[0].source = source;
        // Scaled out of range: refused, and the cloud is as it was.
        let far = ply(&d, "far.ply", &[[2e6, 0.0, 0.0], [2e6, 1.0, 0.0]], false);
        let far = s.execute("pointcloud.insert", &json!({"path": far})).unwrap()["cloud"].as_u64().unwrap();
        let before = s.doc.point_clouds.clone();
        assert!(s.execute("pointcloud.edit", &json!({"cloud": far, "units": "m"})).is_err());
        assert_eq!(s.doc.point_clouds, before);
        let _ = std::fs::remove_dir_all(d);
    }
}
