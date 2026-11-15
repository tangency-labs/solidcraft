//! Links the browser build with an 8 MiB stack: solidcore's kernel algorithms (subdivision,
//! intersection, filleting) recurse legitimately, and wasm has no guard page to catch an
//! overflow (solidcore's README recommends 8 MiB).

fn main() {
    if std::env::var("CARGO_CFG_TARGET_ARCH").is_ok_and(|a| a == "wasm32") {
        println!("cargo:rustc-link-arg=-zstack-size=8388608");
    }
}
