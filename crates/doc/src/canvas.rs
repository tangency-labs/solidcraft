//! Canvases: reference images placed on a plane (Insert Canvas), kept in the design.
//! The image file travels inside the design, as a part of the container of its own, so it
//! reopens without the original. The design holds its bytes, never text.

use serde::{Deserialize, Serialize};
use solidcore_record::{Plane, Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::PlaneRef;

/// Largest image a canvas may hold (bytes).
pub const MAX_CANVAS_BYTES: usize = 16 << 20;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Canvas {
    pub id: u64,
    pub name: String,
    /// `png` or `jpeg`.
    pub format: String,
    /// The image file's bytes. Shared rather than owned: every edit clones the document (the undo
    /// history keeps the old one), and an image of up to 16 MiB copied into each of the 200 undo
    /// states would hold gigabytes. A container writes them to a part of their own; a plain JSON
    /// design can't hold them ([`crate::Document::binary_data`]), and an older one's base64 is
    /// decoded as it opens.
    #[serde(default, skip_serializing, deserialize_with = "crate::blob::lenient")]
    pub data: crate::blob::Blob,
    /// Pixel size of the image.
    pub pixels: [u32; 2],
    pub plane: PlaneRef,
    /// Centre of the image on the plane (plane coordinates, mm).
    pub center: Vec2,
    /// Width on the plane (mm); the height follows the image's aspect.
    pub width: f64,
    /// Rotation on the plane (radians).
    #[serde(default)]
    pub angle: f64,
    #[serde(default = "half")]
    pub opacity: f64,
    /// Mirrored left to right.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
    #[serde(default = "yes")]
    pub visible: bool,
    /// Locked (#12310): its place, size, angle and flip stay as they are, and it is not deleted,
    /// until it is unlocked. Its opacity, visibility and name can still change.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub locked: bool,
    /// The component it belongs to (its plane is in that component's frame, so it moves with
    /// the component's occurrence); 0 = the root.
    #[serde(default, skip_serializing_if = "is_root")]
    pub component: u64,
    /// The sketch points its corners follow, so dimensions and constraints in that sketch place
    /// and size the image. `None`: its own centre, width and angle place it, as they also do
    /// while the sketch or a point is missing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<CanvasAnchor>,
}

/// Sketch points a canvas's corners follow: its bottom-left and bottom-right corners, and its
/// top-left corner when the image may stretch; without it the height follows the image's shape.
/// The points are those of the solved sketch, so the image moves as the sketch solves.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CanvasAnchor {
    /// The sketch's feature id.
    pub sketch: u64,
    /// Point ids in that sketch.
    pub bottom_left: String,
    pub bottom_right: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_left: Option<String>,
}

fn is_root(c: &u64) -> bool {
    *c == 0
}

fn half() -> f64 {
    0.5
}
fn yes() -> bool {
    true
}

impl Canvas {
    pub fn height(&self) -> f64 {
        let [w, h] = self.pixels;
        if w == 0 { self.width } else { self.width * f64::from(h) / f64::from(w) }
    }
    /// The four corners on the plane (plane coordinates): bottom-left, bottom-right, top-right,
    /// top-left of the image.
    pub fn corners(&self) -> [Vec2; 4] {
        let (hw, hh) = (self.width / 2.0, self.height() / 2.0);
        let (s, c) = self.angle.sin_cos();
        let rot = |x: f64, y: f64| self.center + Vec2::new(x * c - y * s, x * s + y * c);
        let f = if self.flip { -1.0 } else { 1.0 };
        [rot(-hw * f, -hh), rot(hw * f, -hh), rot(hw * f, hh), rot(-hw * f, hh)]
    }
    /// Corners on `plane` (in whatever frame the plane is given).
    pub fn world_corners(&self, plane: &Plane) -> [Vec3; 4] {
        self.corners().map(|p| plane.to_world(p))
    }
    /// The corners, in the order of [`Canvas::corners`], of the frame whose bottom edge runs from
    /// `bl` to `br` and whose top-left corner is `tl`; without `tl`, the frame is as high as the
    /// image's shape makes it, its top to the left of the bottom edge's direction. Mirrored left
    /// to right like the canvas. `None` for a frame with no area.
    pub fn frame_corners(&self, bl: Vec2, br: Vec2, tl: Option<Vec2>) -> Option<[Vec2; 4]> {
        let [pw, ph] = self.pixels;
        let aspect = if pw == 0 { 1.0 } else { f64::from(ph) / f64::from(pw) };
        // The kernel's frame: both edges long enough and not along one line.
        let xy = |p: Vec2| [p.x, p.y];
        let [bl, br, tr, tl] = solidcore_ops::affine::picture_frame(xy(bl), xy(br), tl.map(xy), aspect)?.map(|[x, y]| Vec2::new(x, y));
        Some(if self.flip { [br, bl, tl, tr] } else { [bl, br, tr, tl] })
    }
    /// The image file.
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }
}

impl crate::Document {
    /// A canvas's plane in the world: resolved in its component's frame, then placed where the
    /// component's occurrence puts it. Use the evaluated state so planes built from sketch
    /// points, paths or other model geometry resolve as they do when the canvas is inserted.
    /// Face attachments follow the current face in the same frame as attached sketches.
    pub fn canvas_plane(
        &self,
        vals: &std::collections::BTreeMap<String, crate::expr::Value>,
        c: &Canvas,
        state: &crate::ModelState,
    ) -> crate::Result<Plane> {
        let p = if let Some(anchor) = crate::face_anchor::plane_face_anchor(&c.plane) {
            // These wrappers need only the resolved face, so avoid copying the model every frame.
            let mut base = &c.plane;
            while let PlaneRef::Offset { base: next, .. }
            | PlaneRef::Flipped { base: next, .. }
            | PlaneRef::AtAngle { base: next, line: None, pivot: None, .. } = base
            {
                base = next;
            }
            let framed = if matches!(base, PlaneRef::Face { .. }) {
                std::borrow::Cow::Borrowed(state)
            } else {
                crate::state_in_face_anchor_frame(self, state, anchor, c.component)?
            };
            self.resolve_attached_plane(vals, &c.plane, c.component, state, &framed)?
        } else {
            self.resolve_plane_in(vals, &c.plane, 0, c.component, Some(state))?
        };
        let m = self.component_transform(c.component);
        if crate::is_identity(&m) {
            return Ok(p);
        }
        Ok(Plane::new(crate::apply_point(&m, p.origin), crate::apply_vector(&m, p.x), crate::apply_vector(&m, p.y)).unwrap_or(p))
    }

    /// A canvas's corners in the world: the bottom-left, bottom-right, top-right and top-left of
    /// the image. An anchored canvas follows its sketch's solved points; while they give no frame
    /// (the sketch or a point is gone), its own centre, width and angle place it.
    pub fn canvas_corners(
        &self,
        vals: &std::collections::BTreeMap<String, crate::expr::Value>,
        c: &Canvas,
        state: &crate::ModelState,
    ) -> crate::Result<[Vec3; 4]> {
        if let Some(anchor) = &c.anchor
            && let Ok(corners) = self.canvas_anchor_corners(c, anchor, state)
        {
            return Ok(corners);
        }
        let plane = self.canvas_plane(vals, c, state)?;
        Ok(c.world_corners(&plane))
    }

    /// The world corners `anchor` gives `c` (as [`Document::canvas_corners`] orders them), or why
    /// it gives none.
    pub fn canvas_anchor_corners(&self, c: &Canvas, anchor: &CanvasAnchor, state: &crate::ModelState) -> crate::Result<[Vec3; 4]> {
        let corners = self.canvas_anchor_frame(c, anchor, state)?;
        let ss = state.sketch(anchor.sketch).ok_or_else(|| crate::DocError::Invalid(format!("sketch {} is not there", anchor.sketch)))?;
        // The sketch's points are in its component's frame; the world is where that component's
        // occurrence puts it, as for the canvas's own plane.
        let m = self.component_transform(self.feature(anchor.sketch).map_or(c.component, |f| f.component));
        let placed = |p: Vec2| {
            let w = ss.plane.to_world(p);
            if crate::is_identity(&m) { w } else { crate::apply_point(&m, w) }
        };
        Ok(corners.map(placed))
    }

    /// The corners `anchor` gives `c` (as [`Document::canvas_corners`] orders them) in the
    /// coordinates of the anchor's sketch, or why it gives none.
    pub fn canvas_anchor_frame(&self, c: &Canvas, anchor: &CanvasAnchor, state: &crate::ModelState) -> crate::Result<[Vec2; 4]> {
        let invalid = |m: String| crate::DocError::Invalid(m);
        let ss = state.sketch(anchor.sketch).ok_or_else(|| invalid(format!("{} follows sketch {}, which is not there", c.name, anchor.sketch)))?;
        let point = |id: &str| {
            ss.sketch.points.iter().find(|p| p.id == id).map(|p| p.pos).ok_or_else(|| invalid(format!("sketch `{}` has no point `{id}`", ss.name)))
        };
        let (bl, br) = (point(&anchor.bottom_left)?, point(&anchor.bottom_right)?);
        let tl = anchor.top_left.as_deref().map(point).transpose()?;
        c.frame_corners(bl, br, tl).ok_or_else(|| {
            invalid(format!(
                "the corners {} follows in sketch `{}` make a frame with no area: its points coincide or lie on one line",
                c.name, ss.name
            ))
        })
    }
}

/// Image kind and pixel size from the file header (PNG or JPEG).
pub fn image_info(b: &[u8]) -> Option<(&'static str, [u32; 2])> {
    if b.len() > 24 && b.starts_with(b"\x89PNG\r\n\x1a\n") {
        let w = u32::from_be_bytes([*b.get(16)?, *b.get(17)?, *b.get(18)?, *b.get(19)?]);
        let h = u32::from_be_bytes([*b.get(20)?, *b.get(21)?, *b.get(22)?, *b.get(23)?]);
        return (w > 0 && h > 0).then_some(("png", [w, h]));
    }
    if b.starts_with(&[0xFF, 0xD8]) {
        // Walk the markers to a start-of-frame.
        let mut i = 2;
        while i + 9 < b.len() {
            if *b.get(i)? != 0xFF {
                i += 1;
                continue;
            }
            let m = *b.get(i + 1)?;
            if m == 0xD8 || m == 0x01 || (0xD0..=0xD7).contains(&m) || m == 0xFF {
                i += if m == 0xFF { 1 } else { 2 };
                continue;
            }
            let len = usize::from(u16::from_be_bytes([*b.get(i + 2)?, *b.get(i + 3)?]));
            if matches!(m, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
                let h = u32::from(u16::from_be_bytes([*b.get(i + 5)?, *b.get(i + 6)?]));
                let w = u32::from(u16::from_be_bytes([*b.get(i + 7)?, *b.get(i + 8)?]));
                return (w > 0 && h > 0).then_some(("jpeg", [w, h]));
            }
            i += 2 + len;
        }
    }
    None
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(b: &[u8]) -> String {
    let mut out = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        for k in 0..4 {
            if k <= c.len() {
                out.push(char::from(B64[((n >> (18 - 6 * k)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for ch in s.bytes() {
        let v = match ch {
            b'A'..=b'Z' => ch - b'A',
            b'a'..=b'z' => ch - b'a' + 26,
            b'0'..=b'9' => ch - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xFF) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trip_and_headers() {
        for n in 0..10 {
            let b: Vec<u8> = (0..n).map(|i| (i * 37) as u8).collect();
            assert_eq!(base64_decode(&base64_encode(&b)).unwrap(), b);
        }
        assert_eq!(base64_encode(b"Man"), "TWFu");
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(640u32.to_be_bytes());
        png.extend(480u32.to_be_bytes());
        png.extend([8, 6, 0, 0, 0]);
        assert_eq!(image_info(&png), Some(("png", [640, 480])));
        let jpg = [0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0, 0xFF, 0xC0, 0, 11, 8, 0, 100, 0, 200, 3, 0, 0, 0];
        assert_eq!(image_info(&jpg), Some(("jpeg", [200, 100])));
        assert_eq!(image_info(b"nope"), None);
    }

    fn canvas(pixels: [u32; 2], flip: bool) -> Canvas {
        Canvas {
            id: 1,
            name: "Decal1".into(),
            format: "png".into(),
            data: b"bytes".into(),
            pixels,
            plane: PlaneRef::Origin { name: "XY".into() },
            center: Vec2::new(0.0, 0.0),
            width: 10.0,
            angle: 0.0,
            opacity: 1.0,
            flip,
            visible: true,
            locked: false,
            component: 0,
            anchor: None,
        }
    }

    /// A frame from its bottom edge takes its height from the image (2:1 here), its top to the
    /// left of the edge's direction; a top-left corner given stretches it; mirroring swaps the
    /// image's left and right.
    #[test]
    fn a_frame_follows_its_bottom_edge_and_the_image_shape_or_its_top_corner() {
        let (bl, br) = (Vec2::new(10.0, 5.0), Vec2::new(10.0, 25.0));
        // Edge 20 mm along +y: "up" is -x, half as high as wide for a 200 x 100 image.
        assert_eq!(canvas([200, 100], false).frame_corners(bl, br, None), Some([bl, br, Vec2::new(0.0, 25.0), Vec2::new(0.0, 5.0)]));
        let tl = Vec2::new(-30.0, 5.0);
        assert_eq!(canvas([200, 100], false).frame_corners(bl, br, Some(tl)), Some([bl, br, Vec2::new(-30.0, 25.0), tl]));
        assert_eq!(
            canvas([200, 100], true).frame_corners(bl, br, Some(tl)),
            Some([br, bl, tl, Vec2::new(-30.0, 25.0)]),
            "mirrored: the image's bottom-left is the frame's bottom-right"
        );
        // No area: coincident bottom corners, or a top corner on the bottom edge's line.
        assert_eq!(canvas([200, 100], false).frame_corners(bl, bl, None), None);
        assert_eq!(canvas([200, 100], false).frame_corners(bl, br, Some(Vec2::new(10.0, 40.0))), None);
        assert_eq!(canvas([200, 100], false).frame_corners(bl, Vec2::new(f64::NAN, 0.0), None), None);
    }
}
