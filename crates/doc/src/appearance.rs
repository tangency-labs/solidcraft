//! Appearances: how bodies, faces and components look (colour and opacity, with a name), kept
//! in the design. A face's appearance wins over its body's, a body's over its component's, and
//! a component's over the physical material's look; a body with none of these keeps the colour
//! it was imported with.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use solidcraft_geom::Vec3;

use crate::Document;

/// A look: a name (a library entry's, or "Custom"), sRGB colour and opacity (1 = opaque).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "LookWire")]
pub struct Look {
    pub name: String,
    pub color: [u8; 3],
    #[serde(default = "opaque")]
    pub opacity: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pbr: Option<crate::pbr_appearance::PbrParameters>,
}

#[derive(Deserialize)]
struct LookWire {
    name: String,
    color: [u8; 3],
    #[serde(default = "opaque")]
    opacity: f64,
    #[serde(default)]
    pbr: Option<crate::pbr_appearance::PbrParameters>,
}

impl TryFrom<LookWire> for Look {
    type Error = String;
    fn try_from(value: LookWire) -> std::result::Result<Self, Self::Error> {
        if value.pbr.is_some() && value.opacity != 1.0 {
            return Err("opaque PBR requires opacity 1; refraction/transmission is not supported".into());
        }
        Ok(Self { name: value.name, color: value.color, opacity: value.opacity, pbr: value.pbr })
    }
}

fn opaque() -> f64 {
    1.0
}

impl Look {
    pub fn custom(color: [u8; 3], opacity: f64) -> Look {
        Look { name: "Custom".into(), color, opacity: opacity.clamp(0.0, 1.0), pbr: None }
    }
    pub fn opaque_surface(&self) -> crate::Result<Option<solidcraft_render::surface::OpaqueSurface>> {
        if self.pbr.is_some() && self.opacity != 1.0 {
            return Err(crate::DocError::Invalid("opaque PBR requires opacity 1".into()));
        }
        self.pbr.map(|p| p.surface(self.color).map_err(|e| crate::DocError::Invalid(e.to_string()))).transpose()
    }
    /// Colour as 0–1 fractions.
    pub fn rgb(&self) -> [f32; 3] {
        self.color.map(|x| x as f32 / 255.0)
    }
    pub fn hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.color[0], self.color[1], self.color[2])
    }
}

/// A face's appearance: the face of `body` through `point` (world, on the face).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceLook {
    pub body: String,
    pub point: Vec3,
    pub look: Look,
    /// The face's persistent name (found first).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Strict semantic assignments never transfer to a nearby replacement face.
    #[serde(default, skip_serializing_if = "is_false")]
    pub strict_name: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub certificate: Option<crate::config::FaceCertificate>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// The look a body was imported with, carried through later features (which build new kernel
/// bodies without it): its colour and opacity, and its faces' colours by persistent face name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportedLook {
    pub color: Option<[f32; 3]>,
    pub opacity: Option<f32>,
    pub faces: BTreeMap<String, ([f32; 3], f32)>,
}

impl ImportedLook {
    /// A body's imported look: its own kernel paint, else what it carries.
    pub fn of(b: &crate::ModelBody) -> Option<std::sync::Arc<ImportedLook>> {
        if b.body.paint().is_none() && b.body.color().is_none() {
            return b.imported.clone();
        }
        let names = crate::naming::face_names(b);
        let faces = b
            .body
            .paint()
            .map(|p| {
                p.faces.iter().filter_map(|f| names.get(f.face).map(|n| (crate::naming::strip_piece(n).to_string(), (f.color, f.opacity)))).collect()
            })
            .unwrap_or_default();
        Some(std::sync::Arc::new(ImportedLook { color: b.body.color(), opacity: b.body.paint().map(|p| p.opacity), faces }))
    }
}

/// The design's appearances.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Appearances {
    /// By body name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bodies: BTreeMap<String, Look>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<FaceLook>,
    /// By component id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub components: BTreeMap<u64, Look>,
}

impl Appearances {
    pub fn is_empty(&self) -> bool {
        self.bodies.is_empty() && self.faces.is_empty() && self.components.is_empty()
    }

    /// Bound bulk appearance operations, including designs read from untrusted files.
    pub fn check_budget(&self) -> crate::Result<()> {
        if self.bodies.len().saturating_add(self.faces.len()).saturating_add(self.components.len()) > 100_000 {
            return Err(crate::DocError::Invalid("at most 100000 appearance overrides can be processed".into()));
        }
        Ok(())
    }

    /// Build a replacement without changing the source. Equality includes the label and
    /// every optical value. Physical-material defaults are deliberately outside this scope.
    /// `None` means no change, allowing callers to preserve history and revision.
    pub fn replacing(&self, from: &Look, to: &Look, scope: Option<&AppearanceScope>) -> crate::Result<Option<(Self, usize)>> {
        self.check_budget()?;
        for look in [from, to] {
            if !look.opacity.is_finite() || !(0.0..=1.0).contains(&look.opacity) {
                return Err(crate::DocError::Invalid("opacity must be finite and in 0..1".into()));
            }
            look.opaque_surface()?;
        }
        if from == to {
            return Ok(None);
        }
        let body_in_scope = |body: &str| scope.is_none_or(|s| s.bodies.contains(body));
        let component_in_scope = |id: &u64| scope.is_none_or(|s| s.components.contains(id));
        let count = self.bodies.iter().filter(|(body, look)| body_in_scope(body) && *look == from).count()
            + self.faces.iter().filter(|face| body_in_scope(&face.body) && &face.look == from).count()
            + self.components.iter().filter(|(id, look)| component_in_scope(id) && *look == from).count();
        if count == 0 {
            return Ok(None);
        }
        let mut replaced = self.clone();
        for (body, look) in &mut replaced.bodies {
            if body_in_scope(body) && look == from {
                *look = to.clone();
            }
        }
        for face in &mut replaced.faces {
            if body_in_scope(&face.body) && &face.look == from {
                face.look = to.clone();
            }
        }
        for (id, look) in &mut replaced.components {
            if component_in_scope(id) && look == from {
                *look = to.clone();
            }
        }
        Ok(Some((replaced, count)))
    }
}

/// Exact override containers for replacement. A body includes its face overrides;
/// a component selects its own override, independently of the bodies it owns.
#[derive(Clone, Debug, Default)]
pub struct AppearanceScope {
    pub bodies: BTreeSet<String>,
    pub components: BTreeSet<u64>,
}

/// A target in an evaluated model. Face indices are those of `model.faces`.
#[derive(Clone, Copy, Debug)]
pub enum AppearanceTarget<'a> {
    Body(&'a str),
    Face { body: &'a str, face: usize },
    Component(u64),
}

/// The four override steps, in descending precedence. Imported paint is a display
/// fallback, not an override or physical material, and is not a fifth step.
#[derive(Clone, Debug, PartialEq)]
pub struct AppearanceChain {
    pub face: Option<Look>,
    pub body: Option<Look>,
    pub component: Option<Look>,
    pub material: Option<Look>,
}

impl AppearanceChain {
    pub fn steps(&self) -> [(&'static str, Option<&Look>); 4] {
        [("face", self.face.as_ref()), ("body", self.body.as_ref()), ("component", self.component.as_ref()), ("material", self.material.as_ref())]
    }

    pub fn active(&self) -> Option<&'static str> {
        self.steps().into_iter().find_map(|(step, look)| look.map(|_| step))
    }

    pub fn resolved(&self) -> Option<&Look> {
        self.steps().into_iter().find_map(|(_, look)| look)
    }
}

/// The built-in appearance library: (name, colour, opacity).
#[path = "../../../assets/appearances/library.rs"]
mod defaults;
pub use defaults::LIBRARY;
#[path = "../../../assets/appearances/opaque_finishes.rs"]
mod finishes;
pub use finishes::OPAQUE_FINISHES;

/// A library appearance by name (case-insensitive).
pub fn library(name: &str) -> Option<Look> {
    LIBRARY
        .iter()
        .find(|(n, _, _)| n.eq_ignore_ascii_case(name.trim()))
        .map(|(n, c, o)| Look { name: n.to_string(), color: *c, opacity: *o, pbr: None })
        .or_else(|| {
            let (name, color, metallic, roughness) = OPAQUE_FINISHES.iter().find(|(n, ..)| n.eq_ignore_ascii_case(name.trim()))?;
            let pbr = crate::pbr_appearance::PbrParameters::new(*metallic, *roughness, 1.5).ok()?;
            Some(Look { name: (*name).to_string(), color: *color, opacity: 1.0, pbr: Some(pbr) })
        })
        .or_else(|| crate::appearance_pack::find(name)?.opaque_look().ok())
}

/// The look a physical material gives a body that has no appearance of its own.
pub fn material_look(material: &str) -> Option<Look> {
    let color = crate::material_color(material)?;
    let opacity = if material == "Glass" { 0.25 } else { 1.0 };
    Some(Look { name: material.to_string(), color, opacity, pbr: None })
}

impl Document {
    /// Read the same precedence used by body and face display resolution. Components
    /// have no material assignment of their own: the material step is present only
    /// when their directly owned bodies all have the same material look. Empty or
    /// mixed components have no material step. Root is not an assignable component.
    pub fn appearance_chain(&self, state: &crate::ModelState, target: AppearanceTarget<'_>) -> crate::Result<AppearanceChain> {
        self.appearances.check_budget()?;
        match target {
            AppearanceTarget::Body(name) | AppearanceTarget::Face { body: name, .. } => {
                let body = state.body(name).ok_or_else(|| crate::DocError::Invalid(format!("no body `{name}`")))?;
                let mut chain = self.body_appearance_chain(body.definition_name(), self.model_body_component(body));
                if let AppearanceTarget::Face { face, .. } = target {
                    if face >= body.body.face_count() {
                        return Err(crate::DocError::Invalid("unknown face index".into()));
                    }
                    let mut overrides = Vec::new();
                    self.apply_face_overrides(body, &mut overrides);
                    chain.face = overrides.into_iter().find_map(|(index, look)| (index == face).then_some(look));
                }
                Ok(chain)
            }
            AppearanceTarget::Component(id) => {
                if id == 0 || !self.components.iter().any(|c| c.id == id) {
                    return Err(crate::DocError::Invalid("unknown or root component".into()));
                }
                if state.bodies.len() > 100_000 {
                    return Err(crate::DocError::Invalid("too many bodies to read a component appearance chain".into()));
                }
                // Legacy ownership can refer to a feature. Index it once, rather
                // than scanning the timeline for each body of a large component.
                let features: BTreeMap<_, _> = self.features.iter().rev().map(|f| (f.id, f.component)).collect();
                let owner = |b: &&crate::ModelBody| {
                    if b.transferred || b.source_name.is_some() {
                        b.component
                    } else {
                        self.body_components.get(&b.name).copied().or_else(|| features.get(&b.feature).copied()).unwrap_or(0)
                    }
                };
                let mut materials = state
                    .bodies
                    .iter()
                    .filter(|b| owner(b) == id)
                    .map(|b| self.materials.get(b.definition_name()).and_then(|m| self.material_look(m)));
                let material = materials.next().flatten().filter(|first| materials.all(|look| look.as_ref() == Some(first)));
                Ok(AppearanceChain { face: None, body: None, component: self.appearances.components.get(&id).cloned(), material })
            }
        }
    }

    fn body_appearance_chain(&self, name: &str, component: u64) -> AppearanceChain {
        AppearanceChain {
            face: None,
            body: self.appearances.bodies.get(name).cloned(),
            component: (component != 0).then(|| self.appearances.components.get(&component).cloned()).flatten(),
            material: self.materials.get(name).and_then(|material| self.material_look(material)),
        }
    }

    /// Resolve a picked face through the same appearance chain used by the viewport.
    pub fn model_face_look(&self, body: &crate::ModelBody, face: usize) -> Option<Look> {
        self.face_colors(body).into_iter().find(|(index, _)| *index == face).map(|(_, look)| look).or_else(|| self.model_display_look(body))
    }

    /// Body appearance including the imported fallback used by `body_color`.
    pub fn model_display_look(&self, body: &crate::ModelBody) -> Option<Look> {
        self.model_body_look(body).or_else(|| {
            let color = body.body.color().or_else(|| body.imported.as_ref().and_then(|p| p.color))?;
            Some(Look::custom(color.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8), self.model_body_opacity(body)))
        })
    }

    /// Distinct appearances used by an object, including its face overrides. `None` is
    /// the unassigned/imported body appearance. Repeated faces count as one object user.
    pub fn model_used_looks(&self, body: &crate::ModelBody) -> Vec<Option<Look>> {
        let faces = self.face_colors(body);
        let mut looks = Vec::new();
        if faces.len() < body.body.face_count() || faces.is_empty() {
            looks.push(self.model_display_look(body));
        }
        for (_, look) in faces {
            let look = Some(look);
            if !looks.contains(&look) {
                looks.push(look);
            }
        }
        looks
    }

    /// Shared definition appearance for a computed occurrence body.
    pub fn model_body_look(&self, body: &crate::ModelBody) -> Option<Look> {
        self.body_appearance_chain(body.definition_name(), self.model_body_component(body)).resolved().cloned()
    }
    /// Effective body opacity, including paint carried from an imported body.
    pub fn model_body_opacity(&self, body: &crate::ModelBody) -> f64 {
        self.model_body_look(body)
            .map(|look| look.opacity)
            .or_else(|| body.body.paint().map(|paint| paint.opacity as f64))
            .or_else(|| body.imported.as_ref().and_then(|paint| paint.opacity.map(f64::from)))
            .unwrap_or(1.0)
    }
    /// A body's look (without face appearances): its own, else its component's, else its
    /// material's.
    pub fn body_look(&self, name: &str, feature: u64) -> Option<Look> {
        self.body_appearance_chain(name, self.body_component(name, feature)).resolved().cloned()
    }

    /// Face appearances on a body: (point on the face, look).
    pub fn face_looks(&self, body: &str) -> Vec<(Vec3, Look)> {
        self.appearances.faces.iter().filter(|f| f.body == body).map(|f| (f.point, f.look.clone())).collect()
    }

    /// The faces of a body with appearances of their own: (B-rep face index, look). A later
    /// assignment to the same face wins.
    pub fn face_colors(&self, b: &crate::ModelBody) -> Vec<(usize, Look)> {
        let mut out = self.imported_face_colors(b);
        for (index, look) in self.authored_face_colors(b) {
            out.retain(|(old, _)| *old != index);
            out.push((index, look));
        }
        out
    }

    /// Imported paint alone, after applying body-appearance precedence.
    pub(crate) fn imported_face_colors(&self, b: &crate::ModelBody) -> Vec<(usize, Look)> {
        // An assigned body appearance replaces imported paint; design face appearances still win.
        let to8 = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
        if self.appearances.bodies.contains_key(b.definition_name()) {
            Vec::new()
        } else {
            match b.body.paint() {
                Some(p) => p.faces.iter().map(|f| (f.face, Look::custom(f.color.map(to8), f.opacity as f64))).collect(),
                // Carried from an earlier version of the body: by face name (worked out only here:
                // the viewport asks every body for its face colours on every change).
                None => match &b.imported {
                    Some(imp) => crate::naming::face_names(b)
                        .iter()
                        .enumerate()
                        .filter_map(|(i, n)| imp.faces.get(crate::naming::strip_piece(n)).map(|(c, o)| (i, Look::custom(c.map(to8), *o as f64))))
                        .collect(),
                    None => Vec::new(),
                },
            }
        }
    }

    /// Design-authored face overrides, without imported STEP paint. Linked designs carry
    /// these independently so an inherited body look does not hide their face overrides.
    pub fn authored_face_colors(&self, b: &crate::ModelBody) -> Vec<(usize, Look)> {
        let mut out = Vec::new();
        self.apply_face_overrides(b, &mut out);
        out
    }

    fn apply_face_overrides(&self, b: &crate::ModelBody, out: &mut Vec<(usize, Look)>) {
        for f in self.appearances.faces.iter().filter(|f| f.body == b.definition_name()) {
            let (p, look) = (crate::apply_point(&b.source_transform, f.point), f.look.clone());
            if f.strict_name {
                let Some(name) = f.name.as_ref() else { continue };
                let Some(origin_feature) = crate::persistent_name::made_by(name) else {
                    continue;
                };
                let target = crate::config::ConfigFaceTarget {
                    body: f.body.clone(),
                    origin_feature,
                    name: crate::naming::strip_piece(name).into(),
                    point: f.point,
                    certificate: f.certificate.clone(),
                };
                let Ok(indices) = self.configuration_face_indices(b, &target) else { continue };
                for index in indices {
                    out.retain(|(old, _)| *old != index);
                    out.push((index, look.clone()));
                }
                continue;
            }
            // By name (every piece of a split face), else by the point.
            if let Some(n) = &f.name {
                let hits = crate::naming::faces_with_stem(b, n);
                if !hits.is_empty() {
                    for i in hits {
                        out.retain(|(j, _)| *j != i);
                        out.push((i, look.clone()));
                    }
                    continue;
                }
            }
            if !f.strict_name
                && let Some(i) = face_index_at(b, p)
            {
                out.retain(|(j, _)| *j != i);
                out.push((i, look));
            }
        }
    }
}

/// The B-rep face of a body nearest a point (within 5% of the body's size).
pub fn face_index_at(b: &crate::ModelBody, p: Vec3) -> Option<usize> {
    let (d, f) = nearest_face(b, p)?;
    (d <= pick_tolerance(b)).then_some(f)
}

/// The face of `b` nearest `p` and how far it is, however far.
pub fn nearest_face(b: &crate::ModelBody, p: Vec3) -> Option<(f64, usize)> {
    let m = b.mesh();
    let mut best: Option<(f64, u32)> = None;
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        let Some([a, bb, c]) = m.tri(t) else { continue };
        let d = closest_on_triangle(p, a, bb, c).dist(p);
        if best.is_none_or(|(x, _)| d < x) {
            best = Some((d, *f));
        }
    }
    best.map(|(d, f)| (d, f as usize))
}

/// How far from a face of `b` a picked point may lie and still pick it (`face_index_at`).
pub fn pick_tolerance(b: &crate::ModelBody) -> f64 {
    b.body.size() * 0.05 + 1e-6
}

/// The point of triangle abc nearest p.
pub(crate) fn closest_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let den = va + vb + vc;
    if den.abs() < 1e-300 {
        return a;
    }
    a + ab * (vb / den) + ac * (vc / den)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only an appearance change is appearance-only: with any other field changed too, or with
    /// nothing changed, the model must be evaluated as usual.
    #[test]
    fn appearance_only_changes_are_proved_on_the_whole_document() {
        let before = crate::Document::default();
        let mut painted = before.clone();
        painted.appearances.bodies.insert("Body1".into(), Look::custom([255, 0, 0], 0.5));
        assert!(painted.differs_only_in_appearance(&before));
        assert!(before.differs_only_in_appearance(&painted), "clearing it too");
        assert!(!before.differs_only_in_appearance(&before.clone()), "nothing changed");
        let mut mixed = painted.clone();
        mixed.materials.insert("Body1".into(), "Steel".into());
        assert!(!mixed.differs_only_in_appearance(&before), "a material change is not appearance-only");
    }

    fn imported_box() -> crate::ModelBody {
        let body = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(10., 10., 10.)).unwrap();
        let faces = body
            .faces(0.01)
            .unwrap()
            .iter()
            .map(|face| solidcraft_kernel::FacePaint { face: face.index, color: [1., 0., 0.], opacity: 0.5 })
            .collect();
        crate::ModelBody::new("Imported".into(), body.with_paint(Some(solidcraft_kernel::Paint { opacity: 1., faces })), 1)
    }

    fn check_imported_appearance_precedence(body: &crate::ModelBody) {
        let mut doc = Document::default();
        let face_count = body.body.faces(0.01).unwrap().len();
        let imported = doc.face_colors(body);
        assert_eq!(imported.len(), face_count);
        assert!(imported.iter().all(|(_, look)| *look == Look::custom([255, 0, 0], 0.5)));

        let body_look = Look::custom([0, 0, 255], 0.8);
        doc.appearances.bodies.insert(body.definition_name().into(), body_look.clone());
        assert_eq!(doc.model_body_look(body), Some(body_look.clone()));
        assert_eq!(doc.model_body_opacity(body), body_look.opacity);
        assert!(doc.face_colors(body).is_empty(), "all faces must inherit the assigned body appearance");

        let face_look = Look::custom([0, 255, 0], 1.);
        let name = crate::naming::face_names(body)[0].clone();
        doc.appearances.faces.push(FaceLook {
            body: body.definition_name().into(),
            point: Vec3::ZERO,
            look: face_look.clone(),
            name: Some(name),
            strict_name: false,
            certificate: None,
        });
        assert_eq!(doc.face_colors(body), vec![(0, face_look.clone())]);

        doc.appearances.bodies.clear();
        let restored = doc.face_colors(body);
        assert_eq!(restored.len(), face_count);
        assert!(restored.iter().all(|(face, look)| *look == if *face == 0 { face_look.clone() } else { Look::custom([255, 0, 0], 0.5) }));
    }

    #[test]
    fn linked_paint_retains_imported_faces_without_baking_authored_overrides() {
        let mut state = crate::ModelState::default();
        state.bodies.push(imported_box());
        let body = &state.bodies[0];
        let mut doc = Document::default();
        doc.appearances.faces.push(FaceLook {
            body: body.name.clone(),
            point: Vec3::ZERO,
            look: Look::custom([0, 255, 0], 1.0),
            name: Some(crate::naming::face_names(body)[0].clone()),
            strict_name: false,
            certificate: None,
        });
        let exported = doc.painted(&state);
        assert!(exported.bodies[0].body.paint().unwrap().faces.iter().any(|face| face.color == [0., 1., 0.]));
        let linked = doc.painted_for_link(&state);
        let faces = &linked.bodies[0].body.paint().unwrap().faces;
        assert_eq!(faces.len(), body.body.faces(0.01).unwrap().len());
        assert!(faces.iter().all(|face| face.color == [1., 0., 0.] && face.opacity == 0.5));
    }

    #[test]
    fn body_appearance_overrides_imported_face_paint() {
        check_imported_appearance_precedence(&imported_box());
    }

    #[test]
    fn body_appearance_overrides_carried_imported_face_paint() {
        let mut body = imported_box();
        body.imported = ImportedLook::of(&body);
        body.body = body.body.clone().with_paint(None);
        body.source_name = Some(body.name.clone());
        body.name = "Occurrence".into();
        check_imported_appearance_precedence(&body);
    }

    fn chain_fixture() -> (Document, crate::ModelState) {
        let mut doc = Document::new("Appearance chain");
        doc.components.push(crate::Component { id: 1, name: "Part".into(), ..Default::default() });
        doc.body_components.insert("B".into(), 1);
        let mut state = crate::ModelState::default();
        state.bodies.push(crate::ModelBody::new("B".into(), solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(10., 10., 10.)).unwrap(), 1));
        (doc, state)
    }

    #[test]
    fn appearance_chain_agrees_with_display_and_definition_identity() {
        let (mut doc, mut state) = chain_fixture();
        let body_look = library("ABS - Red").unwrap();
        let face_look = library("Steel - Satin PBR").unwrap();
        doc.appearances.bodies.insert("B".into(), body_look.clone());
        doc.appearances.components.insert(1, library("ABS - Blue").unwrap());
        doc.materials.insert("B".into(), "Brass".into());
        let body = &state.bodies[0];
        let spot = crate::naming::face_spots(body)[0].unwrap().point;
        let face_name = crate::naming::face_names(body)[0].clone();
        doc.appearances.faces.push(FaceLook {
            body: "B".into(),
            point: spot,
            look: face_look.clone(),
            name: Some(face_name),
            strict_name: false,
            certificate: None,
        });
        let chain = doc.appearance_chain(&state, AppearanceTarget::Face { body: "B", face: 0 }).unwrap();
        assert_eq!(chain.active(), Some("face"));
        assert_eq!(chain.resolved(), Some(&face_look));
        assert_eq!(doc.model_face_look(body, 0).as_ref(), chain.resolved());
        let chain = doc.appearance_chain(&state, AppearanceTarget::Body("B")).unwrap();
        assert_eq!(chain.active(), Some("body"));
        assert_eq!(doc.model_body_look(body).as_ref(), chain.resolved());
        assert_eq!(doc.body_look("B", 1).as_ref(), chain.resolved());
        assert_eq!(chain.component.as_ref().unwrap().name, "ABS - Blue");
        assert_eq!(chain.material.as_ref().unwrap().name, "Brass");
        // A computed placement resolves the definition's assignments, not its label.
        state.bodies[0].name = "Occurrence/B".into();
        state.bodies[0].source_name = Some("B".into());
        state.bodies[0].component = 1;
        let chain = doc.appearance_chain(&state, AppearanceTarget::Body("Occurrence/B")).unwrap();
        assert_eq!(chain.body, Some(body_look));
        assert_eq!(chain.material.as_ref().unwrap().name, "Brass");
        assert!(doc.appearance_chain(&state, AppearanceTarget::Face { body: "Occurrence/B", face: usize::MAX }).is_err());
        assert!(doc.appearance_chain(&state, AppearanceTarget::Body("missing")).is_err());
    }

    #[test]
    fn appearance_chain_component_material_is_common_or_absent_and_imports_stay_fallbacks() {
        let (mut doc, mut state) = chain_fixture();
        state.bodies[0].imported = Some(std::sync::Arc::new(ImportedLook { color: Some([0.2, 0.4, 0.6]), ..Default::default() }));
        let body = doc.appearance_chain(&state, AppearanceTarget::Body("B")).unwrap();
        assert_eq!(body.active(), None);
        assert!(doc.model_display_look(&state.bodies[0]).is_some());
        doc.materials.insert("B".into(), "Brass".into());
        assert_eq!(doc.appearance_chain(&state, AppearanceTarget::Component(1)).unwrap().active(), Some("material"));
        let mut second = state.bodies[0].clone();
        second.name = "C".into();
        doc.body_components.insert("C".into(), 1);
        state.bodies.push(second);
        assert_eq!(doc.appearance_chain(&state, AppearanceTarget::Component(1)).unwrap().active(), None);
        doc.materials.insert("C".into(), "Brass".into());
        assert_eq!(doc.appearance_chain(&state, AppearanceTarget::Component(1)).unwrap().active(), Some("material"));
        doc.materials.insert("C".into(), "Steel".into());
        assert_eq!(doc.appearance_chain(&state, AppearanceTarget::Component(1)).unwrap().material, None);
        doc.appearances.components.insert(1, library("ABS - Blue").unwrap());
        assert_eq!(doc.appearance_chain(&state, AppearanceTarget::Component(1)).unwrap().active(), Some("component"));
        state.bodies.clear();
        assert_eq!(doc.appearance_chain(&state, AppearanceTarget::Component(1)).unwrap().material, None);
        assert!(doc.appearance_chain(&state, AppearanceTarget::Component(0)).is_err());
        assert!(doc.appearance_chain(&state, AppearanceTarget::Component(99)).is_err());
    }

    #[test]
    fn appearance_replacement_validates_values_and_design_budget_without_mutation() {
        let from = library("ABS - Red").unwrap();
        let to = library("ABS - Blue").unwrap();
        let mut appearances = Appearances::default();
        appearances.bodies.insert("B".into(), from.clone());
        for opacity in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1., 2.] {
            let mut invalid = to.clone();
            invalid.opacity = opacity;
            assert!(appearances.replacing(&from, &invalid, None).is_err());
            assert!(appearances.replacing(&invalid, &to, None).is_err());
            assert_eq!(appearances.bodies["B"], from);
        }
        let mut invalid = library("Steel - Satin PBR").unwrap();
        invalid.opacity = 0.5;
        assert!(appearances.replacing(&from, &invalid, None).is_err());
        let (changed, count) = appearances.replacing(&from, &to, None).unwrap().unwrap();
        assert_eq!(count, 1);
        assert_eq!(changed.bodies["B"], to);
        assert_eq!(appearances.bodies["B"], from);
        assert!(appearances.replacing(&from, &from, None).unwrap().is_none());
        for i in 0..100_000 {
            appearances.components.insert(i, from.clone());
        }
        assert!(appearances.replacing(&from, &to, None).is_err());
        let (mut doc, state) = chain_fixture();
        doc.appearances = appearances;
        assert!(doc.appearance_chain(&state, AppearanceTarget::Body("B")).is_err());
    }

    #[test]
    fn appearance_chain_of_ten_thousand_body_component_has_four_steps() {
        let (mut doc, mut state) = chain_fixture();
        let mut body = state.bodies[0].clone();
        body.source_name = Some("B".into());
        body.component = 1;
        state.bodies = (0..10_000)
            .map(|i| {
                let mut b = body.clone();
                b.name = format!("Placed {i}");
                b
            })
            .collect();
        doc.materials.insert("B".into(), "Brass".into());
        let chain = doc.appearance_chain(&state, AppearanceTarget::Component(1)).unwrap();
        assert_eq!(chain.steps().len(), 4);
        assert_eq!(chain.active(), Some("material"));
    }

    #[test]
    fn library_and_materials() {
        assert_eq!(library("abs - red").map(|l| l.color), Some([196, 40, 40]));
        assert!(library("Glass - Clear").is_some_and(|l| l.opacity < 0.5));
        assert!(library("nope").is_none());
        assert_eq!(material_look("Brass").map(|l| l.color), crate::material_color("Brass"));
        assert!(material_look("Glass").is_some_and(|l| l.opacity < 1.0));
        assert_eq!(Look::custom([1, 2, 3], 7.0).opacity, 1.0);
        assert_eq!(Look::custom([255, 0, 16], 1.0).hex(), "#ff0010");
    }
    mod starter_pack {
        use crate::appearance::Look;
        use crate::appearance_pack::*;
        use std::collections::BTreeSet;

        #[test]
        fn stable_ids_categories_and_original_licences_cover_the_requested_pack() {
            let mut ids = BTreeSet::new();
            let mut names = BTreeSet::new();
            for preset in STARTER_PACK {
                assert!(preset.id.starts_with("starter.") && preset.id.len() <= 128);
                assert!(preset.id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-'));
                assert!(ids.insert(preset.id));
                assert!(names.insert(preset.name.to_ascii_lowercase()));
                assert_eq!(preset.revision, 1);
                assert_eq!(preset.provenance.license, "MIT OR Apache-2.0");
                assert!(!preset.provenance.author.is_empty());
                assert_eq!(preset.provenance.source, "assets/appearances/starter_pack.rs");
                assert_eq!(find(preset.id), Some(preset));
                assert_eq!(find(preset.name), Some(preset));
            }
            for (category, count) in [
                (Category::Wood, 8),
                (Category::Metal, 10),
                (Category::Plastic, 10),
                (Category::Glass, 6),
                (Category::Solar, 2),
                (Category::Regolith, 4),
            ] {
                assert!(STARTER_PACK.iter().filter(|p| p.category == category).count() >= count);
            }
        }

        #[test]
        fn opaque_construction_and_owned_look_roundtrip_preserve_current_response() {
            let mut admitted = 0;
            for preset in STARTER_PACK {
                if let Recipe::Opaque(recipe) = preset.recipe {
                    let look = preset.opaque_look().unwrap();
                    assert_eq!(look.opacity, 1.0);
                    assert_eq!(look.color, recipe.color);
                    let surface = look.opaque_surface().unwrap().unwrap();
                    assert_eq!(surface.metallic(), recipe.metallic);
                    assert_eq!(surface.roughness(), recipe.roughness);
                    let json = serde_json::to_value(&look).unwrap();
                    assert_eq!(serde_json::from_value::<Look>(json.clone()).unwrap(), look);
                    assert!(json.get("provenance").is_none() && json.get("requirements").is_none() && json.get("id").is_none());
                    assert_eq!(preset.requirements, &[Requirement::OpaquePbr]);
                    admitted += 1;
                }
            }
            assert!(admitted >= 34);
        }

        #[test]
        fn glass_transmission_recipes_are_valid_but_never_scalar_or_alpha_looks() {
            for preset in STARTER_PACK.iter().filter(|p| p.category == Category::Glass) {
                let Recipe::Glass(recipe) = preset.recipe else { panic!("glass category must carry a glass recipe") };
                recipe.validate().unwrap();
                assert!(!preset.is_opaque());
                assert!(preset.opaque_look().is_err());
                assert_eq!(preset.requirements, &[Requirement::Refraction, Requirement::CertifiedThinOrClosed]);
                // Compatibility of illustrative values only: the real optics adapter must
                // derive and admit these absorption values after geometry certification.
                assert!(recipe.transmission_linear_rgb.iter().all(|t| (-t.ln() / recipe.reference_path_mm) <= 100.0));
                for channel in [0.0, -1.0, 1.01, f64::NAN, f64::INFINITY] {
                    let mut bad = recipe;
                    bad.transmission_linear_rgb[0] = channel;
                    assert!(bad.validate().is_err());
                }
                let mut bad = recipe;
                bad.reference_path_mm = 0.0;
                assert!(bad.validate().is_err());
                bad = recipe;
                bad.boundary = GlassBoundary::Thin { thickness_mm: 10001.0 };
                assert!(bad.validate().is_err());
            }
            // The swatch shows the transmission as sRGB codes.
            assert_eq!(find("starter.glass.smoky-neutral").unwrap().preview_color(), [160, 166, 173]);
        }

        #[test]
        fn legacy_library_and_six_existing_finish_names_are_not_overridden_or_duplicated() {
            assert!(!crate::appearance::LIBRARY.is_empty());
            let mut names = BTreeSet::new();
            for &(name, color, opacity) in crate::appearance::LIBRARY {
                assert!(!name.is_empty() && names.insert(name.to_ascii_lowercase()));
                let look = crate::appearance::library(name).unwrap();
                assert_eq!(look.color, color);
                assert_eq!(look.opacity, opacity);
                assert!(look.pbr.is_none());
                assert!(find(name).is_none());
            }
            for name in [
                "Steel - Satin PBR",
                "Aluminum - Satin PBR",
                "Copper - Clean PBR",
                "ABS - Molded PBR",
                "PLA - Smooth PBR",
                "Wood - Oak Uniform PBR Preview",
            ] {
                assert!(STARTER_PACK.iter().all(|p| !p.name.eq_ignore_ascii_case(name)), "existing #1366 finish must remain independent");
            }
        }

        #[test]
        fn uniform_previews_and_planned_resource_scale_are_explicit_and_not_bindings() {
            let mut recipes = 0;
            for preset in STARTER_PACK {
                if let Some(recipe) = preset.future_texture_recipe {
                    assert!(preset.name.contains("Uniform Preview"));
                    assert!(preset.preview_note.contains("Uniform opaque preview only"));
                    assert_eq!(recipe.id, format!("{}.maps-v1", preset.id));
                    assert!(recipe.repeat_mm.iter().all(|v| v.is_finite() && *v > 0.0 && *v <= 10000.0));
                    assert_eq!(recipe.grain_axis, if preset.category == Category::Wood { Some(TextureAxis::V) } else { None });
                    assert_eq!(preset.requirements, &[Requirement::OpaquePbr]);
                    recipes += 1;
                }
            }
            assert!(recipes >= 14);
        }
    }
}
