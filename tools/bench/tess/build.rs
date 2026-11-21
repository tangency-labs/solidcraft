// SolidCraft's own mesher lived in crates/geom/src/tess.rs until the one mesher moved to solidcore-tess.
fn main() {
    println!("cargo:rustc-check-cfg=cfg(old_geom_tess)");
    println!("cargo:rerun-if-changed=../../../crates/geom/src/tess.rs");
    if std::path::Path::new("../../../crates/geom/src/tess.rs").exists() {
        println!("cargo:rustc-cfg=old_geom_tess");
    }
}
