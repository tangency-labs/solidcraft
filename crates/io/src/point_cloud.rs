//! Point cloud files read for a design's point cloud (#5151): PLY ([`crate::ply`]) and XYZ text
//! (`x y z` per line, optionally followed by `r g b`, or by an intensity and then `r g b` as PTS
//! files write it; spaces, tabs, commas or semicolons between the values). A file with more
//! points than asked for is thinned evenly (every n-th point). The points are kept relative to
//! the first one, in single precision.

use solidcraft_doc::point_cloud::{MAX_COORD, MAX_POINTS};
use solidcraft_geom::Vec3;

use crate::{IoError, Result};

/// Largest point cloud file read.
pub const MAX_POINT_CLOUD_BYTES: usize = 1 << 30;
/// Points kept from a file unless asked for another number.
pub const DEFAULT_MAX_POINTS: usize = 5_000_000;
/// Is this a point cloud file we read (`.ply`, `.xyz`, `.pts`, any case)?
pub fn is_point_cloud_path(path: &str) -> bool {
    std::path::Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|e| ["ply", "xyz", "pts"].iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// Is this a PLY file (`.ply`, any case)?
pub fn is_ply_path(path: &str) -> bool {
    std::path::Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("ply"))
}

/// A point cloud as read from a file, in millimetres.
#[derive(Debug, Default)]
pub struct CloudFile {
    /// The first point kept: the others are relative to it.
    pub origin: Vec3,
    /// x, y, z per point, from `origin`.
    pub points: Vec<f32>,
    /// r, g, b per point, or empty when the file has no colours.
    pub colors: Vec<u8>,
    /// Points in the file.
    pub source_points: u64,
    /// The file's other properties of the points (PLY).
    pub complex: solidcraft_doc::point_cloud::Complex,
    pub warnings: Vec<String>,
}

/// Points kept as they are read.
struct Keep {
    scale: f64,
    origin: Option<Vec3>,
    out: CloudFile,
    colored: bool,
    bad: u64,
    max: usize,
}

impl Keep {
    fn push(&mut self, [x, y, z]: [f64; 3], rgb: Option<[u8; 3]>) {
        let p = Vec3::new(x, y, z) * self.scale;
        if ![p.x, p.y, p.z].iter().all(|v| v.is_finite() && v.abs() <= MAX_COORD) || self.out.points.len() / 3 >= self.max {
            self.bad += 1;
            return;
        }
        let o = *self.origin.get_or_insert(p);
        let d = p - o;
        self.out.points.extend([d.x as f32, d.y as f32, d.z as f32]);
        if self.colored {
            self.out.colors.extend(rgb.unwrap_or([255; 3]));
        }
    }
}

/// Read a PLY or XYZ file (by `file`'s extension; a file starting with `ply` is PLY whatever its
/// name) whose lengths are `scale` millimetres each, keeping at most `max_points` points.
pub fn read_point_cloud(bytes: &[u8], file: &str, scale: f64, max_points: usize) -> Result<CloudFile> {
    if bytes.len() > MAX_POINT_CLOUD_BYTES {
        return Err(IoError::Invalid(format!("point cloud file too large ({} MB, limit {} MB)", bytes.len() >> 20, MAX_POINT_CLOUD_BYTES >> 20)));
    }
    if !(scale.is_finite() && scale > 0.0 && scale <= 1e6) {
        return Err(IoError::Invalid("the point cloud's scale must be positive".into()));
    }
    let max = max_points.clamp(1, MAX_POINTS);
    let mut keep = Keep { scale, origin: None, out: CloudFile::default(), colored: false, bad: 0, max };
    let ply = crate::ply::is_ply(bytes) || (is_ply_path(file) && !bytes.is_empty());
    let stride;
    if ply {
        let r = crate::ply_complex::read_ply_cloud(bytes, max)?;
        stride = r.vertices.div_ceil(r.positions.len().max(1) as u64).max(1);
        keep.colored = r.colors.is_some();
        let colors = r.colors.unwrap_or_default();
        for (i, p) in r.positions.iter().enumerate() {
            keep.push(*p, colors.get(i).copied());
        }
        // A point left out would shift every number after it.
        if keep.bad > 0 && !r.complex.is_empty() {
            return Err(IoError::Invalid(format!("{} point(s) are not finite numbers within range", keep.bad)));
        }
        keep.out.source_points = r.vertices;
        keep.out.complex = r.complex;
    } else {
        stride = read_xyz(bytes, &mut keep)?;
    }
    let mut out = keep.out;
    if out.points.is_empty() {
        return Err(IoError::Invalid("the file has no points SolidCraft can read".into()));
    }
    if keep.bad > 0 {
        out.warnings.push(format!("{} point(s) could not be read and were left out", keep.bad));
    }
    if stride > 1 {
        out.warnings.push(format!("{} of the file's {} points kept (one in {stride})", out.points.len() / 3, out.source_points));
    }
    out.origin = keep.origin.unwrap_or(Vec3::ZERO);
    Ok(out)
}

/// The numbers of an XYZ line, or `None` for a line that isn't data.
fn numbers(line: &str, out: &mut Vec<f64>) -> bool {
    out.clear();
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
        return false;
    }
    for tok in line.split([' ', '\t', ',', ';']).filter(|t| !t.is_empty()) {
        match tok.parse::<f64>() {
            Ok(v) if out.len() < 16 => out.push(v),
            Ok(_) => {}
            Err(_) => return false,
        }
    }
    true
}

/// Read an XYZ file into `keep`; returns the thinning stride.
fn read_xyz(bytes: &[u8], keep: &mut Keep) -> Result<u64> {
    let text = String::from_utf8_lossy(bytes);
    let lines = bytes.iter().filter(|b| **b == b'\n').count() as u64 + 1;
    let stride = lines.div_ceil(keep.max as u64).max(1);
    let mut v = Vec::with_capacity(16);
    // Where the colour is: after x, y, z (6 numbers), or after an intensity too (7, as PTS).
    let mut rgb_at: Option<Option<usize>> = None;
    let mut data = 0u64;
    for line in text.lines() {
        if !numbers(line, &mut v) {
            if !line.trim().is_empty() && !line.trim_start().starts_with('#') && !line.trim_start().starts_with("//") {
                keep.bad += 1;
            }
            continue;
        }
        // A PTS file's first line is its count.
        if v.len() < 3 {
            if data > 0 || v.len() != 1 {
                keep.bad += 1;
            }
            continue;
        }
        let at = *rgb_at.get_or_insert(match v.len() {
            6 => Some(3),
            n if n >= 7 => Some(4),
            _ => None,
        });
        keep.colored = at.is_some();
        data += 1;
        if !(data - 1).is_multiple_of(stride) {
            continue;
        }
        let rgb = at.and_then(|i| {
            let c = |k: usize| v.get(i + k).filter(|c| c.is_finite()).map(|c| c.round().clamp(0.0, 255.0) as u8);
            Some([c(0)?, c(1)?, c(2)?])
        });
        keep.push([v[0], v[1], v[2]], rgb);
    }
    keep.out.source_points = data;
    Ok(stride)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ply_points(n: usize) -> Vec<u8> {
        let mut b = format!(
            "ply\nformat binary_little_endian 1.0\nelement vertex {n}\nproperty float32 x\nproperty float32 y\nproperty float32 z\nend_header\n"
        )
        .into_bytes();
        for i in 0..n {
            for v in [i as f32, 0.5, 100_000.0] {
                b.extend(v.to_le_bytes());
            }
        }
        b
    }

    #[test]
    fn a_ply_in_metres_is_kept_relative_to_its_first_point_and_thinned() {
        let c = read_point_cloud(&ply_points(10), "scan.ply", 1000.0, 5).unwrap();
        assert_eq!(c.source_points, 10);
        assert_eq!(c.points.len(), 15, "every second point");
        assert_eq!(c.origin, Vec3::new(0.0, 500.0, 1e8));
        assert_eq!(&c.points[3..6], &[2000.0, 0.0, 0.0]);
        assert!(c.colors.is_empty());
        assert_eq!(c.warnings.len(), 1, "{:?}", c.warnings);
        let all = read_point_cloud(&ply_points(10), "scan.PLY", 1.0, DEFAULT_MAX_POINTS).unwrap();
        assert_eq!((all.points.len(), all.warnings.len()), (30, 0));
        assert!(read_point_cloud(&ply_points(10), "scan.ply", 0.0, 5).is_err());
    }

    #[test]
    fn xyz_and_pts_text() {
        let xyz = "# x y z r g b\n1 2 3 255 0 0\n4,5,6,0,255,0\n\n7\t8\t9\t0\t0\t255\nnot a point\n";
        let c = read_point_cloud(xyz.as_bytes(), "a.xyz", 1.0, DEFAULT_MAX_POINTS).unwrap();
        assert_eq!(c.source_points, 3);
        assert_eq!(c.origin, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(c.points, [0.0, 0.0, 0.0, 3.0, 3.0, 3.0, 6.0, 6.0, 6.0]);
        assert_eq!(c.colors, [255, 0, 0, 0, 255, 0, 0, 0, 255]);
        assert_eq!(c.warnings.len(), 1, "the line that isn't a point");
        let pts = "2\n0 0 0 -500 10 20 30\n1 1 1 -400 40 50 60\n";
        let c = read_point_cloud(pts.as_bytes(), "a.pts", 25.4, DEFAULT_MAX_POINTS).unwrap();
        assert_eq!((c.source_points, c.warnings.len()), (2, 0));
        assert_eq!(c.colors, [10, 20, 30, 40, 50, 60]);
        assert!((c.points[3] - 25.4).abs() < 1e-6);
        let plain = read_point_cloud(b"0 0 0\n1 0 0\n", "a.xyz", 1.0, 1).unwrap();
        assert_eq!(plain.points.len(), 3, "thinned to one point");
    }

    #[test]
    fn hostile_point_clouds_never_panic() {
        for (b, name) in [
            (&b""[..], "a.ply"),
            (b"", "a.xyz"),
            (b"1e400 0 0\n", "a.xyz"),
            (b"nan nan nan\n", "a.xyz"),
            (b"1 2\n3\n", "a.xyz"),
            (b"\xff\xfe\x00", "a.xyz"),
            (b"ply\nformat ascii 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nend_header\n1e300 0 0\n", "a.ply"),
            (b"PLY", "b.ply"),
        ] {
            let r = std::panic::catch_unwind(|| read_point_cloud(b, name, 1000.0, DEFAULT_MAX_POINTS).is_ok());
            assert_eq!(r.ok(), Some(false), "{:?}", String::from_utf8_lossy(b));
        }
        assert!(is_point_cloud_path("x.PLY") && is_point_cloud_path("y.xyz") && is_point_cloud_path("z.pts") && !is_point_cloud_path("w.stl"));
    }
}
