//! Bounded reader for the VRML 2.0 models KiCad footprints reference (`.wrl`).
//!
//! KiCad's standard libraries attach VRML models to footprints, so a board import that only
//! understood STEP silently lost most of its components. This reader understands the subset
//! those models use: `Transform`, `Group`, `Switch`, `Shape`, `Appearance`/`Material`,
//! `IndexedFaceSet` with `Coordinate`/`Color`, and the `Box`, `Sphere`, `Cylinder` and `Cone`
//! primitives. `DEF`/`USE` are resolved, and anything else is skipped with a warning.
//!
//! Coordinates are read as they are stored in millimetres (KiCad models are authored in its
//! 3D frame); the footprint transform is applied by the caller.

use std::collections::HashMap;

use solidcraft_doc::{Mat, mat_mul, rigid};
use solidcraft_geom::Vec3;

use crate::{IoError, Result};

/// Largest VRML file read.
pub const MAX_VRML_BYTES: usize = 64 << 20;
const MAX_TOKENS: usize = 2_000_000;
const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 500_000;
const MAX_VERTICES: usize = 5_000_000;
const MAX_TRIANGLES: usize = 5_000_000;
const MAX_NUMBER: f64 = 1.0e9;
/// Segments in a generated primitive's outline.
const PRIMITIVE_SEGMENTS: usize = 32;

/// A VRML file read as one mesh.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VrmlMesh {
    pub positions: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
    /// The first material's diffuse colour, if the file names one.
    pub color: Option<[f32; 3]>,
    pub warnings: Vec<String>,
}

/// Is this a VRML file name (`.wrl`, any case)?
pub fn is_vrml_path(path: &str) -> bool {
    std::path::Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("wrl"))
}

/// Read a VRML 2.0 model. VRML 1.0 files are refused (they use a different node set).
pub fn read_vrml(bytes: &[u8], file: &str) -> Result<VrmlMesh> {
    if bytes.len() > MAX_VRML_BYTES {
        return Err(IoError::Invalid(format!("VRML model too large ({} MB, limit {} MB)", bytes.len() >> 20, MAX_VRML_BYTES >> 20)));
    }
    let source = std::str::from_utf8(bytes).map_err(|_| IoError::Invalid("VRML model is not UTF-8".into()))?;
    let version = source.lines().find(|l| l.trim_start().starts_with("#VRML")).map(str::trim).unwrap_or("");
    if version.contains("V1.0") {
        return Err(IoError::Invalid("VRML 1.0 models are not supported; convert the model to VRML 2.0".into()));
    }
    let tokens = tokenize(source)?;
    let mut parser = Parser { tokens, next: 0, nodes: 0, defs: HashMap::new() };
    let mut roots = Vec::new();
    while parser.next < parser.tokens.len() {
        if let Some(node) = parser.node(0)? {
            roots.push(node);
        }
    }
    let mut reader = Reader { mesh: VrmlMesh::default(), bumped: 0 };
    for node in &roots {
        reader.walk(node, &solidcraft_doc::IDENTITY, 0);
    }
    if reader.mesh.positions.is_empty() {
        return Err(IoError::Invalid(format!("VRML model `{file}` has no geometry")));
    }
    Ok(reader.mesh)
}

#[derive(Clone, Debug, PartialEq)]
enum Token<'a> {
    Word(&'a str),
    Number(f64),
    Text(String),
    Open,
    Close,
    Bracket,
    EndBracket,
    Comma,
}

fn tokenize(source: &str) -> Result<Vec<Token<'_>>> {
    let mut tokens = Vec::new();
    let mut i = 0usize;
    while i < source.len() {
        let b = source.as_bytes().get(i).copied().unwrap_or_default();
        match b {
            b'#' => {
                while i < source.len() && source.as_bytes().get(i) != Some(&b'\n') {
                    i += 1;
                }
            }
            b'{' => {
                tokens.push(Token::Open);
                i += 1;
            }
            b'}' => {
                tokens.push(Token::Close);
                i += 1;
            }
            b'[' => {
                tokens.push(Token::Bracket);
                i += 1;
            }
            b']' => {
                tokens.push(Token::EndBracket);
                i += 1;
            }
            b',' => {
                tokens.push(Token::Comma);
                i += 1;
            }
            b'"' => {
                let start = i + 1;
                i = start;
                let mut decoded = None::<String>;
                while i < source.len() {
                    match source.as_bytes().get(i).copied() {
                        Some(b'"') => break,
                        Some(b'\\') => {
                            let out = decoded.get_or_insert_with(|| source.get(start..i).unwrap_or("").to_string());
                            i += 1;
                            let escaped = source
                                .get(i..)
                                .and_then(|s| s.chars().next())
                                .ok_or_else(|| IoError::Invalid("trailing escape in VRML string".into()))?;
                            out.push(match escaped {
                                'n' => '\n',
                                'r' => '\r',
                                't' => '\t',
                                other => other,
                            });
                            i += escaped.len_utf8();
                        }
                        Some(_) => i += 1,
                        None => break,
                    }
                }
                if i >= source.len() {
                    return Err(IoError::Invalid("unterminated string in VRML model".into()));
                }
                let value = decoded.unwrap_or_else(|| source.get(start..i).unwrap_or("").to_string());
                tokens.push(Token::Text(value));
                i += 1;
            }
            b if b.is_ascii_whitespace() => i += 1,
            _ => {
                let start = i;
                while i < source.len()
                    && !source
                        .as_bytes()
                        .get(i)
                        .is_some_and(|b| b.is_ascii_whitespace() || matches!(b, b'{' | b'}' | b'[' | b']' | b',' | b'#' | b'"'))
                {
                    i += 1;
                }
                let word = source.get(start..i).ok_or_else(|| IoError::Invalid("invalid UTF-8 boundary in VRML model".into()))?;
                if let Ok(n) = word.parse::<f64>() {
                    if !n.is_finite() || n.abs() > MAX_NUMBER {
                        return Err(IoError::Invalid(format!("number `{word}` is outside the supported VRML range")));
                    }
                    tokens.push(Token::Number(n));
                } else {
                    tokens.push(Token::Word(word));
                }
            }
        }
        if tokens.len() > MAX_TOKENS {
            return Err(IoError::Invalid(format!("VRML model has more than {MAX_TOKENS} tokens")));
        }
    }
    Ok(tokens)
}

#[derive(Clone, Debug, PartialEq)]
enum Value {
    Number(f64),
    Text(String),
    Word(String),
    List(Vec<Value>),
    Node(Box<Node>),
    Flag,
}

#[derive(Clone, Debug, PartialEq)]
struct Node {
    kind: String,
    fields: Vec<(String, Value)>,
}

impl Node {
    fn field(&self, name: &str) -> Option<&Value> {
        self.fields.iter().rev().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v)
    }
    fn node(&self, name: &str) -> Option<&Node> {
        match self.field(name) {
            Some(Value::Node(n)) => Some(n.as_ref()),
            _ => None,
        }
    }
    fn nodes(&self, name: &str) -> Vec<&Node> {
        match self.field(name) {
            Some(Value::Node(n)) => vec![n.as_ref()],
            Some(Value::List(items)) => items.iter().filter_map(|v| if let Value::Node(n) = v { Some(n.as_ref()) } else { None }).collect(),
            _ => Vec::new(),
        }
    }
    fn numbers(&self, name: &str) -> Option<Vec<f64>> {
        match self.field(name)? {
            Value::Number(n) => Some(vec![*n]),
            Value::List(items) => Some(items.iter().filter_map(|v| if let Value::Number(n) = v { Some(*n) } else { None }).collect()),
            _ => None,
        }
    }
    fn flag(&self, name: &str) -> Option<bool> {
        match self.field(name)? {
            Value::Word(w) | Value::Text(w) => Some(!w.eq_ignore_ascii_case("false")),
            Value::Number(n) => Some(*n != 0.0),
            Value::Flag => Some(true),
            _ => None,
        }
    }
}

struct Parser<'a> {
    tokens: Vec<Token<'a>>,
    next: usize,
    nodes: usize,
    defs: HashMap<String, Node>,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Token<'a>> {
        self.tokens.get(self.next)
    }
    fn take(&mut self) -> Option<Token<'a>> {
        let t = self.tokens.get(self.next).cloned();
        self.next += usize::from(t.is_some());
        t
    }
    fn word(&mut self) -> Option<String> {
        match self.peek() {
            Some(Token::Word(w)) => {
                let w = (*w).to_string();
                self.next += 1;
                Some(w)
            }
            _ => None,
        }
    }

    /// A node, `DEF`/`USE`, or `None` for a stray token that is skipped.
    fn node(&mut self, depth: usize) -> Result<Option<Node>> {
        if depth > MAX_DEPTH {
            return Err(IoError::Invalid(format!("VRML model nesting exceeds {MAX_DEPTH}")));
        }
        self.nodes = self.nodes.saturating_add(1);
        if self.nodes > MAX_NODES {
            return Err(IoError::Invalid(format!("VRML model has more than {MAX_NODES} nodes")));
        }
        let Some(first) = self.word() else {
            self.next += usize::from(self.peek().is_some());
            return Ok(None);
        };
        if first.eq_ignore_ascii_case("DEF") {
            let Some(name) = self.word() else { return Ok(None) };
            let node = self.node(depth + 1)?.ok_or_else(|| IoError::Invalid("DEF without a node in VRML model".into()))?;
            self.defs.insert(name, node.clone());
            return Ok(Some(node));
        }
        if first.eq_ignore_ascii_case("USE") {
            let Some(name) = self.word() else { return Ok(None) };
            return Ok(self.defs.get(&name).cloned());
        }
        if first.eq_ignore_ascii_case("PROTO") || first.eq_ignore_ascii_case("EXTERNPROTO") {
            // Skip the body: brackets and braces are balanced, so consume to the end of the node.
            let mut braces = 0usize;
            while let Some(t) = self.take() {
                match t {
                    Token::Open => braces += 1,
                    Token::Close => {
                        braces = braces.saturating_sub(1);
                        if braces == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            return Ok(None);
        }
        if first.eq_ignore_ascii_case("ROUTE") {
            while self.peek().is_some_and(|t| !matches!(t, Token::Open)) {
                self.next += 1;
            }
            return Ok(None);
        }
        let mut node = Node { kind: first, fields: Vec::new() };
        match self.peek() {
            Some(Token::Open) => {
                self.next += 1;
            }
            // Some primitives may appear bare (`Box { }` always has braces, but a definition
            // may end here); treat a missing body as empty.
            _ => return Ok(Some(node)),
        }
        loop {
            match self.peek() {
                Some(Token::Close) => {
                    self.next += 1;
                    break;
                }
                None => return Err(IoError::Invalid("unterminated node in VRML model".into())),
                _ => {
                    let Some(name) = self.word() else {
                        self.next += 1;
                        continue;
                    };
                    let mut value = self.value(depth + 1)?;
                    // VRML writes multi-component fields as bare numbers (`diffuseColor 1 0 0`,
                    // `translation 1 2 3`), not always as a bracketed list.
                    if let Value::Number(first) = value {
                        let mut values = vec![first];
                        while let Some(Token::Number(n)) = self.peek() {
                            values.push(*n);
                            self.next += 1;
                        }
                        value = if values.len() == 1 { Value::Number(first) } else { Value::List(values.into_iter().map(Value::Number).collect()) };
                    }
                    node.fields.push((name, value));
                }
            }
        }
        Ok(Some(node))
    }

    fn value(&mut self, depth: usize) -> Result<Value> {
        if depth > MAX_DEPTH {
            return Err(IoError::Invalid(format!("VRML model nesting exceeds {MAX_DEPTH}")));
        }
        match self.peek().cloned() {
            Some(Token::Number(n)) => {
                self.next += 1;
                Ok(Value::Number(n))
            }
            Some(Token::Text(t)) => {
                self.next += 1;
                Ok(Value::Text(t))
            }
            Some(Token::Bracket) => {
                self.next += 1;
                let mut items = Vec::new();
                loop {
                    match self.peek() {
                        Some(Token::EndBracket) => {
                            self.next += 1;
                            break;
                        }
                        None => return Err(IoError::Invalid("unterminated list in VRML model".into())),
                        Some(Token::Comma) => self.next += 1,
                        _ => items.push(self.value(depth + 1)?),
                    }
                }
                Ok(Value::List(items))
            }
            Some(Token::Word(_)) => {
                // A node value or a bare word (`TRUE`, `FALSE`, an enum value).
                let starts_node = matches!(self.tokens.get(self.next + 1), Some(Token::Open))
                    || matches!(self.tokens.get(self.next), Some(Token::Word(w)) if w.eq_ignore_ascii_case("DEF") || w.eq_ignore_ascii_case("USE"));
                if starts_node {
                    match self.node(depth + 1)? {
                        Some(n) => Ok(Value::Node(Box::new(n))),
                        None => Ok(Value::Flag),
                    }
                } else {
                    let w = self.word().unwrap_or_default();
                    Ok(Value::Word(w))
                }
            }
            Some(Token::Open) | Some(Token::Close) | Some(Token::EndBracket) | Some(Token::Comma) | None => {
                self.next += usize::from(self.peek().is_some());
                Ok(Value::Flag)
            }
        }
    }
}

struct Reader {
    mesh: VrmlMesh,
    /// Warnings already present, to keep repeated node kinds from flooding the list.
    bumped: usize,
}

impl Reader {
    fn warn(&mut self, message: &str) {
        if self.bumped < 32 && !self.mesh.warnings.iter().any(|w| w == message) {
            self.mesh.warnings.push(message.to_string());
            self.bumped += 1;
        }
    }

    fn walk(&mut self, node: &Node, transform: &Mat, depth: usize) {
        if depth > MAX_DEPTH {
            self.warn("VRML model nesting is too deep; deeper nodes were skipped");
            return;
        }
        match node.kind.to_ascii_lowercase().as_str() {
            "transform" => {
                let mut local = solidcraft_doc::IDENTITY;
                if let Some(scale) = node.numbers("scale") {
                    let s = [scale.first().copied().unwrap_or(1.0), scale.get(1).copied().unwrap_or(1.0), scale.get(2).copied().unwrap_or(1.0)];
                    if s.iter().all(|v| v.is_finite() && *v > 0.0) {
                        local = mat_mul(&diag(s[0], s[1], s[2]), &local);
                    } else {
                        self.warn("VRML model has an invalid scale");
                    }
                }
                if let Some(rotation) = node.numbers("rotation") {
                    let axis = Vec3::new(
                        rotation.first().copied().unwrap_or(0.0),
                        rotation.get(1).copied().unwrap_or(0.0),
                        rotation.get(2).copied().unwrap_or(0.0),
                    );
                    let angle = rotation.get(3).copied().unwrap_or(0.0);
                    if axis.len2() > 1e-12 && angle.is_finite() {
                        local = mat_mul(&rigid(Vec3::ZERO, Vec3::ZERO, axis, angle), &local);
                    }
                }
                if let Some(t) = node.numbers("translation") {
                    let v = Vec3::new(t.first().copied().unwrap_or(0.0), t.get(1).copied().unwrap_or(0.0), t.get(2).copied().unwrap_or(0.0));
                    if v.is_finite() {
                        local = mat_mul(&rigid(v, Vec3::ZERO, Vec3::Z, 0.0), &local);
                    }
                }
                let m = mat_mul(transform, &local);
                for child in node.nodes("children") {
                    self.walk(child, &m, depth + 1);
                }
            }
            "group" | "anchor" | "collision" | "billboard" | "separator" => {
                for child in node.nodes("children") {
                    self.walk(child, transform, depth + 1);
                }
            }
            "switch" => {
                if let Some(child) = node.nodes("choice").into_iter().next() {
                    self.walk(child, transform, depth + 1);
                }
            }
            "shape" => {
                let color = node
                    .node("appearance")
                    .and_then(|a| a.node("material"))
                    .and_then(|m| m.numbers("diffuseColor"))
                    .and_then(|c| Some([*c.first()?, *c.get(1)?, *c.get(2)?]))
                    .filter(|c| c.iter().all(|v| v.is_finite()))
                    .map(|c| [c[0] as f32, c[1] as f32, c[2] as f32]);
                if let Some(c) = color {
                    match self.mesh.color {
                        None => self.mesh.color = Some(c),
                        Some(existing)
                            if (existing[0] - c[0]).abs() > 1e-4 || (existing[1] - c[1]).abs() > 1e-4 || (existing[2] - c[2]).abs() > 1e-4 =>
                        {
                            self.warn("VRML model has several materials; the first colour is used");
                        }
                        _ => {}
                    }
                }
                if let Some(geometry) = node.node("geometry") {
                    self.geometry(geometry, transform, color);
                }
            }
            other => self.warn(&format!("VRML node `{other}` is not supported")),
        }
    }

    fn geometry(&mut self, node: &Node, transform: &Mat, _color: Option<[f32; 3]>) {
        match node.kind.to_ascii_lowercase().as_str() {
            "indexedfaceset" => self.indexed_face_set(node, transform),
            "indexedtriangleset" => self.indexed_triangle_set(node, transform),
            "box" => {
                let size = node.numbers("size").unwrap_or_else(|| vec![2.0, 2.0, 2.0]);
                let s = Vec3::new(size.first().copied().unwrap_or(2.0), size.get(1).copied().unwrap_or(2.0), size.get(2).copied().unwrap_or(2.0));
                self.box_mesh(s * 0.5, transform);
            }
            "sphere" => {
                let r = node.numbers("radius").and_then(|v| v.first().copied()).unwrap_or(1.0);
                self.sphere_mesh(r, transform);
            }
            "cylinder" => {
                let r = node.numbers("radius").and_then(|v| v.first().copied()).unwrap_or(1.0);
                let h = node.numbers("height").and_then(|v| v.first().copied()).unwrap_or(2.0);
                self.cylinder_mesh(r, h, false, transform);
            }
            "cone" => {
                let r = node.numbers("bottomRadius").and_then(|v| v.first().copied()).unwrap_or(1.0);
                let h = node.numbers("height").and_then(|v| v.first().copied()).unwrap_or(2.0);
                self.cylinder_mesh(r, h, true, transform);
            }
            other => self.warn(&format!("VRML geometry `{other}` is not supported")),
        }
    }

    fn indexed_face_set(&mut self, node: &Node, transform: &Mat) {
        let Some(points) = node.node("coord").and_then(|c| c.numbers("point")) else {
            self.warn("VRML IndexedFaceSet has no coordinates");
            return;
        };
        let mut local = Vec::with_capacity(points.len() / 3);
        for chunk in points.as_chunks::<3>().0 {
            let p = Vec3::new(chunk[0], chunk[1], chunk[2]);
            if !p.is_finite() {
                self.warn("VRML model has a non-finite point");
                return;
            }
            local.push(solidcraft_doc::apply_point(transform, p));
        }
        let Some(indices) = node.numbers("coordIndex") else {
            self.warn("VRML IndexedFaceSet has no coordIndex");
            return;
        };
        let ccw = node.flag("ccw").unwrap_or(true);
        let base = self.mesh.positions.len() as u32;
        if self.mesh.positions.len().saturating_add(local.len()) > MAX_VERTICES {
            self.warn("VRML model exceeds the vertex limit");
            return;
        }
        self.mesh.positions.extend(local);
        let mut face = Vec::new();
        for raw in indices {
            let index = raw as i64;
            if index < 0 {
                self.face(&face, base, ccw);
                face.clear();
            } else if index as usize >= points.len() / 3 {
                self.warn("VRML model has an out-of-range index");
                face.clear();
            } else {
                face.push(index as u32);
            }
        }
        self.face(&face, base, ccw);
    }

    fn face(&mut self, face: &[u32], base: u32, ccw: bool) {
        for i in 1..face.len().saturating_sub(1) {
            let Some(a) = face.first().copied() else { break };
            let Some(b) = face.get(i).copied() else { break };
            let Some(c) = face.get(i + 1).copied() else { break };
            let mut t = [a.saturating_add(base), b.saturating_add(base), c.saturating_add(base)];
            if !ccw {
                t.swap(1, 2);
            }
            if self.mesh.triangles.len() >= MAX_TRIANGLES {
                self.warn("VRML model exceeds the triangle limit");
                return;
            }
            self.mesh.triangles.push(t);
        }
    }

    fn indexed_triangle_set(&mut self, node: &Node, transform: &Mat) {
        let Some(points) = node.node("coord").and_then(|c| c.numbers("point")) else { return };
        let Some(indices) = node.numbers("index") else { return };
        let base = self.mesh.positions.len() as u32;
        for chunk in points.as_chunks::<3>().0 {
            self.mesh.positions.push(solidcraft_doc::apply_point(transform, Vec3::new(chunk[0], chunk[1], chunk[2])));
        }
        for tri in indices.as_chunks::<3>().0 {
            self.mesh.triangles.push([tri[0] as u32 + base, tri[1] as u32 + base, tri[2] as u32 + base]);
        }
    }

    fn push_triangle(&mut self, a: Vec3, b: Vec3, c: Vec3, transform: &Mat) {
        if self.mesh.triangles.len() >= MAX_TRIANGLES || self.mesh.positions.len() + 3 > MAX_VERTICES {
            self.warn("VRML model exceeds the mesh limits");
            return;
        }
        let base = self.mesh.positions.len() as u32;
        for p in [a, b, c] {
            self.mesh.positions.push(solidcraft_doc::apply_point(transform, p));
        }
        self.mesh.triangles.push([base, base + 1, base + 2]);
    }

    fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, transform: &Mat) {
        self.push_triangle(a, b, c, transform);
        self.push_triangle(a, c, d, transform);
    }

    fn box_mesh(&mut self, h: Vec3, transform: &Mat) {
        let p = |x: f64, y: f64, z: f64| Vec3::new(x * h.x, y * h.y, z * h.z);
        let (a, b, c, d, e, f, g, i) =
            (p(-1., -1., -1.), p(1., -1., -1.), p(1., 1., -1.), p(-1., 1., -1.), p(-1., -1., 1.), p(1., -1., 1.), p(1., 1., 1.), p(-1., 1., 1.));
        self.quad(a, d, c, b, transform);
        self.quad(e, f, g, i, transform);
        self.quad(a, b, f, e, transform);
        self.quad(b, c, g, f, transform);
        self.quad(c, d, i, g, transform);
        self.quad(d, a, e, i, transform);
    }

    fn sphere_mesh(&mut self, r: f64, transform: &Mat) {
        if !(r.is_finite() && r > 0.0) {
            return;
        }
        let n = PRIMITIVE_SEGMENTS;
        for lat in 0..n {
            let t0 = std::f64::consts::PI * lat as f64 / n as f64;
            let t1 = std::f64::consts::PI * (lat + 1) as f64 / n as f64;
            for lon in 0..n {
                let p0 = std::f64::consts::TAU * lon as f64 / n as f64;
                let p1 = std::f64::consts::TAU * (lon + 1) as f64 / n as f64;
                let at = |t: f64, p: f64| Vec3::new(r * t.sin() * p.cos(), r * t.cos(), r * t.sin() * p.sin());
                self.quad(at(t0, p0), at(t1, p0), at(t1, p1), at(t0, p1), transform);
            }
        }
    }

    fn cylinder_mesh(&mut self, r: f64, h: f64, cone: bool, transform: &Mat) {
        if !(r.is_finite() && r > 0.0 && h.is_finite() && h > 0.0) {
            return;
        }
        let n = PRIMITIVE_SEGMENTS;
        let z0 = -h * 0.5;
        let z1 = h * 0.5;
        let ring = |z: f64, radius: f64, i: usize| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            Vec3::new(radius * a.cos(), z, radius * a.sin())
        };
        for i in 0..n {
            let (a0, a1) = (ring(z0, r, i), ring(z0, r, i + 1));
            let (b0, b1) = if cone { (ring(z1, 0.0, i), ring(z1, 0.0, i + 1)) } else { (ring(z1, r, i), ring(z1, r, i + 1)) };
            self.quad(a0, b0, b1, a1, transform);
            self.push_triangle(Vec3::new(0.0, z0, 0.0), a0, a1, transform);
            if !cone {
                self.push_triangle(Vec3::new(0.0, z1, 0.0), b1, b0, transform);
            }
        }
    }
}

fn diag(x: f64, y: f64, z: f64) -> Mat {
    let mut m = solidcraft_doc::IDENTITY;
    m[0][0] = x;
    m[1][1] = y;
    m[2][2] = z;
    m
}

#[cfg(test)]
#[path = "vrml_in_tests.rs"]
mod tests;
