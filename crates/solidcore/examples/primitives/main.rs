//! `cargo run --example primitives`: builds a box, cylinder, cone and pyramid in pure
//! Rust and proves each is a closed manifold solid with the analytic Euler
//! characteristic, volume, area and bounding box. Exits non-zero if any check fails.
//!
//! No STL is written: upstream's open SMLib has no B-rep face tessellation to a mesh
//! (only `SmApiTessellateBoundaries`), so there is nothing faithful to export.

mod checks;

fn main() -> Result<(), String> {
    let mut failed = 0;
    for spec in checks::specs() {
        println!("== {}", spec.name);
        println!(
            "   analytic: volume {:.12} area {:.12} bbox {:?}",
            spec.volume, spec.area, spec.bbox
        );
        match checks::check(&spec) {
            Ok(r) => {
                let (f, e, v, l) = r.counts;
                println!(
                    "   F {f} E {e} V {v} L {l}  V-E+F = {}",
                    v as i64 - e as i64 + f as i64
                );
                println!("   manifold solid: {}", r.manifold_solid);
                println!("   volume {:.12} area {:.12}", r.volume, r.area);
                println!("   bbox {:?}", r.bbox);
                println!("   OK");
            }
            Err(msg) => {
                failed += 1;
                println!("   FAIL: {msg}");
            }
        }
    }
    println!("STL: not written (no B-rep mesh tessellation in upstream SMLib)");
    if failed == 0 {
        Ok(())
    } else {
        Err(format!("{failed} primitive(s) failed"))
    }
}
