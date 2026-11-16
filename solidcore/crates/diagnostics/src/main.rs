//! `solidcore-diag`: a shaded picture and a tolerance report of one `.smb` body, for
//! diagnosing a kernel case.
//!
//! The body is read with the kernel's own reader (which heals a file older than healer
//! version 10 on read, as `SmBrep::ReadFromFile` does), meshed by `solidcore-tess` the way
//! an app meshes it, and drawn with a z-buffer. Edges whose tolerance is above
//! `--edges-above` are drawn over everything, hidden or not, so a gap shows wherever it
//! is. The report lists every face and edge with its tolerance and the pixel of the
//! picture where a label for it belongs.
//!
//! ```text
//! cargo run -p diagnostics --release -- FILE.smb [--out DIR] [--view AZ,EL] [--size WxH]
//!     [--highlight F,..] [--see-through F,..] [--edges-above TOL] [--binary] [--rebuild] [--wireframe]
//! ```
//!
//! It writes `DIR/<stem>.png` and `DIR/<stem>.json` and prints the largest edge
//! tolerances. `--wireframe` draws only native edges and explicitly reports that no
//! face mesh was attempted; the normal shaded path still reports conversion errors.

mod body;
mod png;
mod render;
mod report;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// What to draw and where to write it.
struct Options {
    /// The `.smb` file.
    file: PathBuf,
    /// The directory the picture and the report go to.
    out: PathBuf,
    /// The view direction.
    view: render::View,
    /// The picture's size in pixels.
    width: usize,
    height: usize,
    /// How faces and edges are drawn.
    style: render::Style,
    /// The file is binary, not ASCII.
    binary: bool,
    /// Rebuild the UV trim curves on read instead of checking and fixing them.
    rebuild: bool,
    /// Draw native edges without converting or meshing faces.
    wireframe: bool,
}

const USAGE: &str = "usage: solidcore-diag FILE.smb [--out DIR] [--view AZ,EL] [--size WxH] \
[--highlight F,..] [--see-through F,..] [--edges-above TOL] [--binary] [--rebuild] [--wireframe]";

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)).and_then(|o| run(&o)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("solidcore-diag: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Reads, meshes, draws and reports.
fn run(o: &Options) -> Result<(), String> {
    let mut body = body::read(&o.file, o.binary, o.rebuild)?;
    let faces = body::faces(&body)?;
    let edges = body::edges(&body)?;
    let mesh = if o.wireframe {
        body::wireframe(&edges)
    } else {
        body::mesh(&mut body, &edges)?
    };
    let (picture, frame) = render::draw(&mesh, &edges, o.view, &o.style, o.width, o.height);
    let stem = o
        .file
        .file_stem()
        .map_or_else(|| "body".to_owned(), |s| s.to_string_lossy().into_owned());
    std::fs::create_dir_all(&o.out).map_err(|e| format!("{}: {e}", o.out.display()))?;
    let png_path = o.out.join(format!("{stem}.png"));
    write(
        &png_path,
        &png::encode(picture.width, picture.height, &picture.rgb),
    )?;
    let report = report::build(&o.file, &body, &faces, &edges, &mesh, o.view, &frame);
    let json_path = o.out.join(format!("{stem}.json"));
    let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    write(&json_path, text.as_bytes())?;
    print!("{}", report::summary(&body, &edges, &mesh));
    println!("wrote {} and {}", png_path.display(), json_path.display());
    Ok(())
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn parse(mut args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut file = None;
    let mut o = Options {
        file: PathBuf::new(),
        out: PathBuf::from("."),
        view: render::View {
            azimuth: -35.0,
            elevation: 30.0,
        },
        width: 1200,
        height: 900,
        style: render::Style {
            highlight: Vec::new(),
            see_through: Vec::new(),
            edges_above: 1e-4,
        },
        binary: false,
        rebuild: false,
        wireframe: false,
    };
    while let Some(a) = args.next() {
        let mut value = || {
            args.next()
                .ok_or_else(|| format!("{a} needs a value\n{USAGE}"))
        };
        match a.as_str() {
            "--out" => o.out = PathBuf::from(value()?),
            "--view" => {
                let [azimuth, elevation] = pair::<f64>(&value()?, ',')?;
                o.view = render::View { azimuth, elevation };
            }
            "--size" => [o.width, o.height] = pair::<usize>(&value()?, 'x')?,
            "--highlight" => o.style.highlight = list(&value()?)?,
            "--see-through" => o.style.see_through = list(&value()?)?,
            "--edges-above" => o.style.edges_above = number(&value()?)?,
            "--binary" => o.binary = true,
            "--rebuild" => o.rebuild = true,
            "--wireframe" => o.wireframe = true,
            "-h" | "--help" => return Err(USAGE.to_owned()),
            _ if a.starts_with('-') => return Err(format!("unknown option {a}\n{USAGE}")),
            _ if file.is_none() => file = Some(PathBuf::from(a)),
            _ => return Err(format!("one file only\n{USAGE}")),
        }
    }
    o.file = file.ok_or_else(|| USAGE.to_owned())?;
    if !(16..=8192).contains(&o.width) || !(16..=8192).contains(&o.height) {
        return Err("--size must be between 16x16 and 8192x8192".to_owned());
    }
    if !o.style.edges_above.is_finite()
        || !o.view.azimuth.is_finite()
        || !o.view.elevation.is_finite()
    {
        return Err("--view and --edges-above must be finite".to_owned());
    }
    Ok(o)
}

fn number<T: std::str::FromStr>(s: &str) -> Result<T, String> {
    s.trim().parse().map_err(|_| format!("not a number: {s}"))
}

fn pair<T: std::str::FromStr>(s: &str, sep: char) -> Result<[T; 2], String> {
    let (a, b) = s
        .split_once(sep)
        .ok_or_else(|| format!("expected two values separated by {sep:?}: {s}"))?;
    Ok([number(a)?, number(b)?])
}

fn list(s: &str) -> Result<Vec<usize>, String> {
    s.split(',')
        .filter(|t| !t.trim().is_empty())
        .map(number)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wireframe is opt-in and preserves the requested camera and edge styling.
    #[test]
    fn wireframe_is_explicit_and_preserves_view_and_edge_tolerance() {
        let default = parse(["body.smb".to_owned()].into_iter()).unwrap();
        assert!(!default.wireframe);
        let options = parse(
            [
                "body.smb",
                "--wireframe",
                "--view",
                "12,34",
                "--edges-above",
                "0.002",
            ]
            .map(str::to_owned)
            .into_iter(),
        )
        .unwrap();
        assert!(options.wireframe);
        assert_eq!(options.view.azimuth, 12.0);
        assert_eq!(options.view.elevation, 34.0);
        assert_eq!(options.style.edges_above, 0.002);
    }

    /// An edge-only image remains visible and reports unattempted face meshing.
    #[test]
    fn wireframe_draws_edges_and_reports_unattempted_faces() {
        let body = body::Body {
            kernel: solidcore::Kernel::default(),
            brep: solidcore::Brep::new_with_infinite_region().unwrap(),
        };
        let edges = vec![body::EdgeInfo {
            index: 7,
            tolerance: 0.002,
            faces: vec![3],
            points: vec![[0.0, 0.0, 0.0], [1.0, 1.0, 0.0]],
        }];
        let faces = vec![body::FaceInfo {
            index: 3,
            surface: "plane",
            tolerance: 0.001,
        }];
        let mesh = body::wireframe(&edges);
        assert!(mesh.triangles.is_empty() && mesh.refused.is_empty());
        let o = parse(["body.smb", "--wireframe"].map(str::to_owned).into_iter()).unwrap();
        let (picture, frame) = render::draw(&mesh, &edges, o.view, &o.style, 64, 64);
        assert_eq!(picture.rgb.len(), 64 * 64 * 3);
        assert!(picture.rgb.iter().any(|&channel| channel < 255));
        let data = report::build(&o.file, &body, &faces, &edges, &mesh, o.view, &frame);
        assert_eq!(data["render_mode"], "wireframe");
        assert_eq!(data["mesh"]["attempted"], false);
        assert_eq!(data["mesh"]["triangles"], 0);
        assert_eq!(data["edges"][0]["tolerance"], 0.002);
        assert_eq!(data["faces"][0]["tolerance"], 0.001);
        assert!(data["faces"][0]["label_pixel"].is_null());
        assert!(report::summary(&body, &edges, &mesh).contains("no face mesh attempted"));
    }
}
