#![forbid(unsafe_code)]

// The CLI evaluates features on its main thread, and Windows gives a main thread 1 MiB of stack
// unless the binary asks for more: the kernel asks for 8 MiB (`KERNEL_STACK` in
// crates/kernel/src/par.rs), and fillet certification overflows 1 MiB (#15438).
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let windows = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows");
    if windows && std::env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|env| env == "msvc") {
        println!("cargo:rustc-link-arg-bins=/STACK:8388608");
    }
}
