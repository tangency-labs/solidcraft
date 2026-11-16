//! The bounded existing sketch-import projection of the shared typed DXF file.
//! Source records retain actual DXF fields. No Geom2 or token cache is stored.

use super::*;
use crate::{DxfFileEntity, DxfVertex};

struct SourceBudget {
    bytes: usize,
    limit: usize,
    work: usize,
}

impl SourceBudget {
    fn new(pair_count: usize, input_bytes: usize) -> Result<Self> {
        // Each numeric pair can produce at most one Vec2-sized payload and
        // retained text is bounded by the admitted input. This derives an
        // allocation allowance from the actual token stream, not caller counts.
        let limit = pair_count
            .checked_mul(32)
            .and_then(|n| input_bytes.checked_mul(2).and_then(|b| n.checked_add(b)))
            .ok_or_else(|| IoError::Invalid("DXF source payload budget overflow".into()))?;
        Ok(Self { bytes: 0, limit, work: 0 })
    }
    fn charge(&mut self, count: usize, item_size: usize) -> Result<()> {
        self.bytes = self
            .bytes
            .checked_add(count.checked_mul(item_size).ok_or_else(|| IoError::Invalid("DXF source payload overflow".into()))?)
            .filter(|n| *n <= self.limit)
            .ok_or_else(|| IoError::Invalid("DXF source payload budget exceeded".into()))?;
        Ok(())
    }
    fn copied(&mut self, count: usize) -> Result<()> {
        visit(&mut self.work, count)
    }
    fn points(&mut self, entity: &Ent, x: i32, y: i32, k: f64) -> Result<Vec<Vec2>> {
        let count = entity.f.iter().filter(|(code, _)| *code == x).count();
        if count != entity.f.iter().filter(|(code, _)| *code == y).count() {
            return Ok(Vec::new());
        }
        self.charge(count, std::mem::size_of::<Vec2>())?;
        self.copied(count)?;
        let mut points = Vec::new();
        points.try_reserve_exact(count).map_err(|_| IoError::Invalid("DXF source point allocation failed".into()))?;
        points.extend(
            entity
                .f
                .iter()
                .filter(|(code, _)| *code == x)
                .zip(entity.f.iter().filter(|(code, _)| *code == y))
                .map(|((_, x), (_, y))| Vec2::new(*x * k, *y * k)),
        );
        Ok(points)
    }
    fn numbers(&mut self, entity: &Ent, code: i32) -> Result<Vec<f64>> {
        let count = entity.f.iter().filter(|(c, _)| *c == code).count();
        self.charge(count, std::mem::size_of::<f64>())?;
        self.copied(count)?;
        let mut numbers = Vec::new();
        numbers.try_reserve_exact(count).map_err(|_| IoError::Invalid("DXF source value allocation failed".into()))?;
        numbers.extend(entity.f.iter().filter(|(c, _)| *c == code).map(|(_, v)| *v));
        Ok(numbers)
    }
}

/// Parse once into the common owned file, under the explicitly named existing
/// sketch-import admission. Strict read_dxf_file admission remains unchanged.
pub(super) fn read(parsed: ParsedDxf, input_bytes: usize) -> Result<DxfFile<DxfShape>> {
    let p = parsed.pairs;
    let mut budget = SourceBudget::new(p.len(), input_bytes)?;
    let k = units_scale(&p);
    let start = p.iter().position(|(c, v)| *c == 2 && v == "ENTITIES").ok_or_else(|| IoError::Invalid("no ENTITIES section".into()))?;
    let mut file = DxfFile::for_sketch_import()?;
    if let Some(bs) = p.iter().position(|(c, v)| *c == 2 && v == "BLOCKS") {
        let mut current: Option<(String, Vec2, Vec<Ent>)> = None;
        for entity in section(&p, bs)? {
            match entity.kind.as_str() {
                "BLOCK" => {
                    let base = Vec2::new(entity.get(10).unwrap_or(0.0) * k, entity.get(20).unwrap_or(0.0) * k);
                    let name = entity.text(2).unwrap_or("");
                    budget.charge(name.len(), 1)?;
                    current = Some((owned_source(name)?, base, Vec::new()));
                }
                "ENDBLK" => {
                    if let Some((name, base, list)) = current.take() {
                        let entities = records(list, k, &mut file, &mut budget)?;
                        file.insert_source_block(name, base, entities)?;
                    }
                }
                _ => {
                    if let Some((_, _, list)) = current.as_mut() {
                        list.try_reserve(1).map_err(|_| IoError::Invalid("DXF block record allocation failed".into()))?;
                        list.push(entity);
                    }
                }
            }
        }
    }
    for entity in records(section(&p, start)?, k, &mut file, &mut budget)? {
        file.push_import_record(entity)?;
    }
    file.set_source_work(budget.work);
    Ok(file)
}

fn owned_source(text: &str) -> Result<String> {
    let mut owned = String::new();
    owned.try_reserve_exact(text.len()).map_err(|_| IoError::Invalid("DXF source string allocation failed".into()))?;
    owned.push_str(text);
    Ok(owned)
}
fn omitted(kind: &str) -> Result<DxfShape> {
    Ok(DxfShape::SourceOmitted { kind: owned_source(kind)? })
}

fn records(entities: Vec<Ent>, k: f64, file: &mut DxfFile<DxfShape>, budget: &mut SourceBudget) -> Result<Vec<DxfFileEntity<DxfShape>>> {
    let mut result = Vec::new();
    result.try_reserve(entities.len()).map_err(|_| IoError::Invalid("DXF typed record allocation failed".into()))?;
    let mut iter = entities.into_iter().peekable();
    while let Some(entity) = iter.next() {
        let pt = |x: f64, y: f64| Vec2::new(x * k, y * k);
        let layer_name = entity.text(8).unwrap_or("0");
        budget.charge(layer_name.len(), 1)?;
        let layer = file.import_layer_reference(layer_name)?;
        let paper = entity.get(67).is_some_and(|v| v != 0.0);
        let geometry = if paper {
            omitted(&entity.kind)?
        } else {
            match entity.kind.as_str() {
                "INSERT" => {
                    let name = entity.text(2).unwrap_or("");
                    budget.charge(name.len(), 1)?;
                    DxfShape::Insert {
                        block: owned_source(name)?,
                        at: pt(entity.get(10).unwrap_or(0.0), entity.get(20).unwrap_or(0.0)),
                        scale: Vec2::new(entity.get(41).unwrap_or(1.0), entity.get(42).unwrap_or(1.0)),
                        angle_deg: entity.get(50).unwrap_or(0.0),
                        columns: entity.get(70).unwrap_or(1.0).clamp(1.0, 10_000.0) as usize,
                        rows: entity.get(71).unwrap_or(1.0).clamp(1.0, 10_000.0) as usize,
                        column_spacing: entity.get(44).unwrap_or(0.0) * k,
                        row_spacing: entity.get(45).unwrap_or(0.0) * k,
                    }
                }
                "LINE" => match (entity.get(10), entity.get(20), entity.get(11), entity.get(21)) {
                    (Some(x), Some(y), Some(a), Some(b)) => DxfShape::Line(pt(x, y), pt(a, b)),
                    _ => omitted("LINE")?,
                },
                "POINT" => match (entity.get(10), entity.get(20)) {
                    (Some(x), Some(y)) => DxfShape::Point(pt(x, y)),
                    _ => omitted("POINT")?,
                },
                "CIRCLE" => match (entity.get(10), entity.get(20), entity.get(40)) {
                    (Some(x), Some(y), Some(r)) => DxfShape::Circle { c: pt(x, y), r: r * k },
                    _ => omitted("CIRCLE")?,
                },
                "ARC" => match (entity.get(10), entity.get(20), entity.get(40), entity.get(50), entity.get(51)) {
                    (Some(x), Some(y), Some(r), Some(a0), Some(a1)) => DxfShape::Arc { c: pt(x, y), r: r * k, a0, a1 },
                    _ => omitted("ARC")?,
                },
                "LWPOLYLINE" => {
                    let ys = budget.numbers(&entity, 20)?;
                    let count = entity.f.iter().filter(|(c, _)| *c == 10).count().min(ys.len());
                    budget.charge(count, std::mem::size_of::<DxfVertex>())?;
                    budget.copied(count)?;
                    let mut vertices: Vec<DxfVertex> = Vec::new();
                    vertices.try_reserve_exact(count).map_err(|_| IoError::Invalid("DXF source vertex allocation failed".into()))?;
                    let mut index = 0;
                    for (code, value) in &entity.f {
                        match *code {
                            10 => {
                                if let Some(y) = ys.get(index) {
                                    vertices.push(DxfVertex { at: pt(*value, *y), bulge: 0.0 });
                                }
                                index += 1;
                            }
                            42 => {
                                if let Some(last) = vertices.last_mut() {
                                    last.bulge = *value;
                                }
                            }
                            _ => {}
                        }
                    }
                    DxfShape::SourcePolyline { vertices, closed: entity.get(70).is_some_and(|v| (v as i64) & 1 == 1) }
                }
                "POLYLINE" => {
                    let mut vertices = Vec::new();
                    while iter.peek().is_some_and(|v| v.kind == "VERTEX") {
                        let Some(vertex) = iter.next() else { break };
                        if let (Some(x), Some(y)) = (vertex.get(10), vertex.get(20)) {
                            budget.charge(1, std::mem::size_of::<DxfVertex>())?;
                            budget.copied(1)?;
                            vertices.try_reserve(1).map_err(|_| IoError::Invalid("DXF source vertex allocation failed".into()))?;
                            vertices.push(DxfVertex { at: pt(x, y), bulge: vertex.get(42).unwrap_or(0.0) });
                        }
                    }
                    if iter.peek().is_some_and(|v| v.kind == "SEQEND") {
                        iter.next();
                    }
                    DxfShape::SourcePolyline { vertices, closed: entity.get(70).is_some_and(|v| (v as i64) & 1 == 1) }
                }
                "ELLIPSE" => match (entity.get(10), entity.get(20), entity.get(11), entity.get(21), entity.get(40)) {
                    (Some(x), Some(y), Some(mx), Some(my), Some(ratio)) => DxfShape::SourceEllipse {
                        c: pt(x, y),
                        major: Vec2::new(mx, my) * k,
                        major_radius: Vec2::new(mx, my).len() * k,
                        ratio,
                        t0: entity.get(41).unwrap_or(0.0),
                        t1: entity.get(42).unwrap_or(std::f64::consts::TAU),
                    },
                    _ => omitted("ELLIPSE")?,
                },
                "SPLINE" => DxfShape::SourceSpline {
                    degree: entity.get(71).unwrap_or(3.0),
                    flags: entity.get(70).unwrap_or(0.0),
                    knots: budget.numbers(&entity, 40)?,
                    ctrl: budget.points(&entity, 10, 20, k)?,
                    fit: budget.points(&entity, 11, 21, k)?,
                    weights: budget.numbers(&entity, 41)?,
                },
                "TEXT" | "MTEXT" => match (entity.get(10), entity.get(20), entity.get(40), entity.text(1)) {
                    (Some(x), Some(y), Some(height), Some(text)) => {
                        budget.charge(text.len(), 1)?;
                        budget.copied(text.len())?;
                        DxfShape::Text { at: pt(x, y), height: height * k, angle_deg: entity.get(50).unwrap_or(0.0), text: paragraph_text(text)? }
                    }
                    _ => omitted(&entity.kind)?,
                },
                _ => omitted(&entity.kind)?,
            }
        };
        // Retain only actual source text; missing or paper-space geometry is
        // still counted by its omitted record without keeping unrelated fields.
        let source = if !paper && matches!(geometry, DxfShape::Text { .. }) {
            let mut bytes = 0;
            let source = layered_source_text(entity, k, &mut bytes)?;
            budget.charge(bytes, 1)?;
            source
        } else {
            None
        };
        result.push(file.import_record(layer, geometry, source)?);
    }
    Ok(result)
}

pub(super) fn visit(work: &mut usize, count: usize) -> Result<()> {
    *work = work.checked_add(count).filter(|n| *n <= MAX_WORK).ok_or_else(|| IoError::Invalid("the drawing is too complex".into()))?;
    Ok(())
}

pub(super) fn push(out: &mut Vec<Geom2>, geometry: Geom2) -> Result<()> {
    if out.len() >= MAX_ENTITIES {
        return Err(IoError::Invalid("the drawing is too complex".into()));
    }
    out.try_reserve(1).map_err(|_| IoError::Invalid("DXF sketch geometry allocation failed".into()))?;
    out.push(geometry);
    Ok(())
}

impl DxfFile<DxfShape> {
    /// Project the actual imported source records for the sketch consumer.
    /// Block expansion and metadata interpretation happen here, once per call.
    pub fn sketch_geometry(&self) -> Result<Vec<Geom2>> {
        project(self)
    }
}

pub(super) fn project(file: &DxfFile<DxfShape>) -> Result<Vec<Geom2>> {
    if !file.is_sketch_import() {
        return Err(IoError::Invalid("sketch source projection requires SketchImport admission".into()));
    }
    let mut out = Vec::new();
    let mut work = file.source_work();
    project_records(file, file.entities(), 0, &mut out, &mut work)?;
    Ok(out)
}

fn project_records(
    file: &DxfFile<DxfShape>,
    records: &[DxfFileEntity<DxfShape>],
    depth: usize,
    out: &mut Vec<Geom2>,
    work: &mut usize,
) -> Result<()> {
    for entity in records {
        visit(work, 1)?;
        match entity.geometry() {
            DxfShape::SourceOmitted { .. } => {}
            DxfShape::Insert { block, at, scale, angle_deg, columns, rows, column_spacing, row_spacing } => {
                let Some(block) = file.source_block(block) else { continue };
                if depth >= MAX_BLOCK_DEPTH {
                    continue;
                }
                let mut inner = Vec::new();
                project_records(file, block.entities(), depth + 1, &mut inner, work)?;
                if inner.is_empty() {
                    continue;
                }
                let cells = columns.checked_mul(*rows).ok_or_else(|| IoError::Invalid("DXF INSERT array count overflow".into()))?;
                let output = cells.checked_mul(inner.len()).ok_or_else(|| IoError::Invalid("DXF INSERT output count overflow".into()))?;
                if cells > MAX_ENTITIES || output > MAX_ENTITIES || out.len().checked_add(output).is_none_or(|n| n > MAX_ENTITIES) {
                    return Err(IoError::Invalid("the drawing is too complex".into()));
                }
                visit(work, cells)?;
                let rot = angle_deg.to_radians();
                // Translation varies across cells; linear scale/rotation does
                // not. Charge-check every variable copy before the first cell
                // can allocate an expanded spline/text/sample array.
                let transform = Affine::insert(*at, block.base(), scale.x, scale.y, rot);
                let mut per_cell = 0usize;
                for geometry in &inner {
                    per_cell = per_cell
                        .checked_add(transform.copy_work(geometry))
                        .ok_or_else(|| IoError::Invalid("DXF INSERT payload work overflow".into()))?;
                }
                let copied = per_cell.checked_mul(cells).ok_or_else(|| IoError::Invalid("DXF INSERT payload work overflow".into()))?;
                if work.checked_add(copied).is_none_or(|n| n > MAX_WORK) {
                    return Err(IoError::Invalid("the drawing is too complex: INSERT copied payload exceeds work budget".into()));
                }
                out.try_reserve(output).map_err(|_| IoError::Invalid("DXF INSERT output allocation failed".into()))?;
                for r in 0..*rows {
                    for c in 0..*columns {
                        let off = Vec2::from_angle(rot) * (*column_spacing * c as f64) + Vec2::from_angle(rot).perp() * (*row_spacing * r as f64);
                        let transform = Affine::insert(*at + off, block.base(), scale.x, scale.y, rot);
                        for geometry in &inner {
                            transform.apply(geometry, out, work)?;
                        }
                    }
                }
            }
            DxfShape::Line(a, b) => push(out, Geom2::Line(*a, *b))?,
            DxfShape::Point(p) => push(out, Geom2::Point(*p))?,
            DxfShape::Circle { c, r } => push(out, Geom2::Circle(*c, *r))?,
            DxfShape::Arc { c, r, a0, a1 } => {
                push(out, Geom2::Arc { c: *c, a: *c + Vec2::from_angle(a0.to_radians()) * (*r), b: *c + Vec2::from_angle(a1.to_radians()) * (*r) })?
            }
            DxfShape::SourcePolyline { vertices, closed } => {
                let count = if *closed { vertices.len() } else { vertices.len().saturating_sub(1) };
                for i in 0..count {
                    let (Some(a), Some(b)) = (vertices.get(i), vertices.get((i + 1) % vertices.len())) else { continue };
                    if a.at.dist(b.at) < 1e-12 {
                        continue;
                    }
                    if let Some(geometry) = bulge_arc(a.at, b.at, a.bulge) {
                        push(out, geometry)?;
                    }
                }
            }
            DxfShape::SourceEllipse { c, major, major_radius: a, ratio, t0, t1 } => {
                let major = *c + *major;
                let sweep = (*t1 - *t0).rem_euclid(std::f64::consts::TAU);
                if sweep < 1e-9 || std::f64::consts::TAU - sweep < 1e-9 {
                    push(out, Geom2::Ellipse { c: *c, major, minor: *a * *ratio })?;
                } else {
                    let u = (major - *c) / a.max(1e-12);
                    let v = u.perp() * (*a * *ratio);
                    let at = |t: f64| *c + u * (*a * t.cos()) + v * t.sin();
                    let pieces = (sweep / std::f64::consts::FRAC_PI_2 - 1e-9).ceil().clamp(1.0, 4.0) as usize;
                    let step = sweep / pieces as f64;
                    let w = (step / 2.0).cos();
                    for j in 0..pieces {
                        let (from, to) = (*t0 + step * j as f64, *t0 + step * (j + 1) as f64);
                        let mid = (from + to) / 2.0;
                        let apex = *c + (u * (*a * mid.cos()) + v * mid.sin()) / w;
                        push(out, Geom2::Conic { a: at(from), apex, b: at(to), rho: w / (1.0 + w) })?;
                    }
                }
            }
            DxfShape::SourceSpline { degree, ctrl, fit, .. } => {
                if fit.len() >= 2 {
                    push(out, Geom2::Spline { pts: copy_points(fit, work)?, control: false, degree: 3 })?;
                } else if ctrl.len() >= 2 {
                    push(
                        out,
                        Geom2::Spline { pts: copy_points(ctrl, work)?, control: true, degree: degree.clamp(1.0, MAX_BEZIER_DEGREE as f64) as u8 },
                    )?;
                }
            }
            DxfShape::Text { at, height, angle_deg, text } => {
                let raw = entity.source_text().and_then(|s| s.raw_group1());
                let text = if let Some(raw) = raw { legacy_text(raw.trim(), work)? } else { copy_text(text, work)? };
                let at = entity.source_text().and_then(|s| s.first_anchor_mm()).unwrap_or(*at);
                push(out, Geom2::Text { text, at, height: *height, angle: angle_deg.to_radians() })?;
            }
            _ => return Err(IoError::Invalid("not supported yet: this owned DXF shape is not an imported source record".into())),
        }
    }
    Ok(())
}

pub(super) fn copy_points(points: &[Vec2], work: &mut usize) -> Result<Vec<Vec2>> {
    visit(work, points.len())?;
    let mut copied = Vec::new();
    copied.try_reserve_exact(points.len()).map_err(|_| IoError::Invalid("DXF sketch point copy allocation failed".into()))?;
    copied.extend_from_slice(points);
    Ok(copied)
}
pub(super) fn copy_text(text: &str, work: &mut usize) -> Result<String> {
    visit(work, text.len())?;
    let mut copied = String::new();
    copied.try_reserve_exact(text.len()).map_err(|_| IoError::Invalid("DXF sketch text copy allocation failed".into()))?;
    copied.push_str(text);
    Ok(copied)
}
fn legacy_text(text: &str, work: &mut usize) -> Result<String> {
    visit(work, text.len())?;
    paragraph_text(text)
}
fn paragraph_text(text: &str) -> Result<String> {
    let mut copied = String::new();
    copied.try_reserve_exact(text.len()).map_err(|_| IoError::Invalid("DXF sketch text allocation failed".into()))?;
    for (index, part) in text.split("\\P").enumerate() {
        if index > 0 {
            copied.push('\n');
        }
        copied.push_str(part);
    }
    Ok(copied)
}
