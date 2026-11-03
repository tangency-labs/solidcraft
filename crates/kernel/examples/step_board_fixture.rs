//! A board-like assembly for issue #883, the worst case for a large STEP import: every
//! component is its own product with a package body and its own pins (nothing shared).
//!
//! ```sh
//! scripts/fetch-smlib.sh
//! SOLIDCRAFT_STEP_PCB_COMPONENTS=600 SOLIDCRAFT_STEP_PCB_PINS=15 \
//!     cargo run --release -p solidcraft-kernel --features smlib --example step_board_fixture -- board.step
//! ```
//!
//! 600 components of 15 pins give 52 MB. The defaults are 500 and 16; the path defaults to
//! `board.step`.

#[cfg(not(feature = "smlib"))]
fn main() {
    eprintln!("step_board_fixture needs SMLib: run scripts/fetch-smlib.sh, then add --features smlib");
    std::process::exit(2);
}

#[cfg(feature = "smlib")]
fn main() {
    if let Err(e) = run::main() {
        eprintln!("step_board_fixture: {e}");
        std::process::exit(1);
    }
}

#[cfg(feature = "smlib")]
mod run {
    use solidcraft_geom::Vec3;
    use solidcraft_kernel::{Body, ExportBody, ExportProduct, StepHeader, box_solid, cylinder, fillet, step_export_products};

    /// A count from `SOLIDCRAFT_<name>`, or `default` when it is unset.
    fn count(name: &str, default: usize) -> Result<usize, String> {
        solidcraft_geom::env::var(name).map_or(Ok(default), |s| s.parse().map_err(|e| format!("SOLIDCRAFT_{name}: {e}")))
    }

    /// A package: a box with its vertical edges and then its top edges rounded (left sharp where
    /// a round does not build).
    fn package(w: f64, d: f64, h: f64) -> Result<Body, String> {
        let b = box_solid(Vec3::ZERO, Vec3::new(w, d, h)).map_err(|e| e.to_string())?;
        let vertical = [Vec3::new(0.0, 0.0, h / 2.0), Vec3::new(w, 0.0, h / 2.0), Vec3::new(w, d, h / 2.0), Vec3::new(0.0, d, h / 2.0)];
        let top = [Vec3::new(w / 2.0, 0.0, h), Vec3::new(w, d / 2.0, h), Vec3::new(w / 2.0, d, h), Vec3::new(0.0, d / 2.0, h)];
        let b = fillet(&b, &vertical, 0.3).unwrap_or(b);
        Ok(fillet(&b, &top, 0.1).unwrap_or(b))
    }

    pub fn main() -> Result<(), String> {
        let components = count("STEP_PCB_COMPONENTS", 500)?;
        let pins = count("STEP_PCB_PINS", 16)?;
        let path = std::env::args().nth(1).unwrap_or_else(|| "board.step".into());
        let mut owned: Vec<Vec<Body>> = Vec::new();
        for c in 0..components {
            let w = 4.0 + (c % 7) as f64 * 0.37;
            let d = 3.0 + (c % 5) as f64 * 0.29;
            let mut bodies = vec![package(w, d, 1.2 + (c % 3) as f64 * 0.1)?];
            for p in 0..pins {
                let side = if p % 2 == 0 { -0.6 } else { d + 0.6 };
                let x = 0.3 + (p / 2) as f64 * (w - 0.6) / (pins / 2).max(1) as f64 + c as f64 * 1e-4;
                bodies.push(cylinder(Vec3::new(x, side, -1.5), Vec3::Z, 0.25, 2.0 + (p % 3) as f64 * 0.01).map_err(|e| e.to_string())?);
            }
            owned.push(bodies);
        }
        let mut products: Vec<ExportProduct> = owned
            .iter()
            .enumerate()
            .map(|(c, bodies)| ExportProduct {
                name: format!("U{c}"),
                bodies: bodies
                    .iter()
                    .enumerate()
                    .map(|(k, b)| ExportBody { name: format!("U{c}_{k}"), body: b, color: Some([0.2, 0.2, 0.2]) })
                    .collect(),
                children: Vec::new(),
            })
            .collect();
        let side = (components as f64).sqrt().ceil() as usize;
        let children = (0..components)
            .map(|c| {
                let (x, y) = ((c % side) as f64 * 12.0, (c / side) as f64 * 10.0);
                (c, [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [x, y, 1.6, 1.0]], format!("U{c}"))
            })
            .collect();
        products.push(ExportProduct { name: "Board".into(), bodies: Vec::new(), children });
        let root = products.len() - 1;
        let header = StepHeader { file_name: "board.step".into(), ..Default::default() };
        let text = step_export_products(&products, root, &header).map_err(|e| e.to_string())?;
        std::fs::write(&path, &text).map_err(|e| format!("{path}: {e}"))?;
        println!("wrote {path}: {} bytes, {components} components", text.len());
        Ok(())
    }
}
