//! Open CASCADE (OCCT) behind a small C ABI, the `occt_capi` module, and a safe Rust face on the
//! part of it SolidCraft's opt-in OCCT adapter uses (`solidcraft-kernel`'s `backend-occt`).
//!
//! The module is a separate shared library (`libocct_capi.so`, `.dylib`, `occt_capi.dll`) that
//! holds OCCT; Rust links it dynamically and never touches an OCCT header. That keeps the module
//! the replaceable unit LGPL-2.1 section 6 asks for (README.md).
//!
//! When the build script builds the module under `OUT_DIR`, Cargo puts it on the loader path for
//! `cargo run` and `cargo test`. Anything else needs `LD_LIBRARY_PATH` (Linux),
//! `DYLD_FALLBACK_LIBRARY_PATH` (macOS) or `PATH` (Windows) set to `DEP_OCCT_CAPI_LIB_DIR`.

mod raw {
    unsafe extern "C" {
        pub fn occt_abi_version() -> u32;
        pub fn occt_library_version_ptr() -> *const u8;
        pub fn occt_library_version_len() -> usize;
        pub fn occt_last_error_ptr() -> *const u8;
        pub fn occt_last_error_len() -> usize;

        pub fn occt_buffer_ptr(h: u32) -> *const u8;
        pub fn occt_buffer_len(h: u32) -> usize;
        pub fn occt_buffer_free(h: u32);

        pub fn occt_shape_free(h: u32);
        pub fn occt_shape_is_valid(h: u32) -> i32;

        pub fn occt_make_box(x: f64, y: f64, z: f64, dx: f64, dy: f64, dz: f64) -> u32;
        pub fn occt_make_cylinder(px: f64, py: f64, pz: f64, dx: f64, dy: f64, dz: f64, radius: f64, height: f64) -> u32;
        pub fn occt_make_sphere(x: f64, y: f64, z: f64, radius: f64) -> u32;
        pub fn occt_make_torus_about(x: f64, y: f64, z: f64, dx: f64, dy: f64, dz: f64, major: f64, minor: f64) -> u32;

        pub fn occt_fuse(a: u32, b: u32) -> u32;
        pub fn occt_cut(a: u32, b: u32) -> u32;
        pub fn occt_common(a: u32, b: u32) -> u32;
        pub fn occt_fillet_nearest(h: u32, points: *const f64, count: usize, radius: f64) -> u32;

        pub fn occt_volume(h: u32) -> f64;
        pub fn occt_area(h: u32) -> f64;
        pub fn occt_solid_count(h: u32) -> i32;

        pub fn occt_step_write(h: u32) -> u32;
        pub fn occt_step_read(data: *const u8, len: usize) -> u32;
    }
}

/// The ABI these bindings were written for.
pub const ABI_VERSION: u32 = 3;

/// Why the module refused a call, in its own words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// A call's answer.
pub type Result<T> = std::result::Result<T, Error>;

fn last_error() -> Error {
    // SAFETY: the module keeps a thread-local string alive until the next call on this thread.
    let text = unsafe { bytes(raw::occt_last_error_ptr(), raw::occt_last_error_len()) };
    Error(String::from_utf8_lossy(&text).into_owned())
}

/// # Safety
/// `ptr` is null or points at `len` readable bytes.
unsafe fn bytes(ptr: *const u8, len: usize) -> Vec<u8> {
    if ptr.is_null() || len == 0 {
        return Vec::new();
    }
    // SAFETY: the caller's contract.
    unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec()
}

fn shape(h: u32) -> Result<Shape> {
    if h == 0 { Err(last_error()) } else { Ok(Shape(h)) }
}

fn buffer(h: u32) -> Result<Vec<u8>> {
    if h == 0 {
        return Err(last_error());
    }
    // SAFETY: a live buffer handle names `len` bytes at `ptr` until it is freed, just below.
    let out = unsafe { bytes(raw::occt_buffer_ptr(h), raw::occt_buffer_len(h)) };
    // SAFETY: the handle is this function's own, freed once.
    unsafe { raw::occt_buffer_free(h) };
    Ok(out)
}

/// The module's ABI version, to compare with [`ABI_VERSION`].
pub fn abi_version() -> u32 {
    // SAFETY: no arguments, no state.
    unsafe { raw::occt_abi_version() }
}

/// The OCCT version the module holds, such as `8.0.1`.
pub fn library_version() -> String {
    // SAFETY: a static string of the module.
    let text = unsafe { bytes(raw::occt_library_version_ptr(), raw::occt_library_version_len()) };
    String::from_utf8_lossy(&text).into_owned()
}

/// An OCCT shape, held by the module and freed when dropped. The module serialises every call
/// behind one lock, so a shape may be used from any thread.
#[derive(Debug)]
pub struct Shape(u32);

impl Drop for Shape {
    fn drop(&mut self) {
        // SAFETY: the handle is this value's own, freed once.
        unsafe { raw::occt_shape_free(self.0) }
    }
}

/// Which boolean.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boolean {
    /// Material in either shape.
    Fuse,
    /// The first shape with the second taken away.
    Cut,
    /// Material in both shapes.
    Common,
}

impl Shape {
    /// The box from corner `min` with sides `size`.
    pub fn make_box(min: [f64; 3], size: [f64; 3]) -> Result<Shape> {
        // SAFETY: scalars only.
        shape(unsafe { raw::occt_make_box(min[0], min[1], min[2], size[0], size[1], size[2]) })
    }

    /// The cylinder on `base` along `dir`.
    pub fn make_cylinder(base: [f64; 3], dir: [f64; 3], radius: f64, height: f64) -> Result<Shape> {
        // SAFETY: scalars only.
        shape(unsafe { raw::occt_make_cylinder(base[0], base[1], base[2], dir[0], dir[1], dir[2], radius, height) })
    }

    /// The sphere about `center`.
    pub fn make_sphere(center: [f64; 3], radius: f64) -> Result<Shape> {
        // SAFETY: scalars only.
        shape(unsafe { raw::occt_make_sphere(center[0], center[1], center[2], radius) })
    }

    /// The ring torus about the line through `center` along `dir`.
    pub fn make_torus(center: [f64; 3], dir: [f64; 3], major: f64, minor: f64) -> Result<Shape> {
        // SAFETY: scalars only.
        shape(unsafe { raw::occt_make_torus_about(center[0], center[1], center[2], dir[0], dir[1], dir[2], major, minor) })
    }

    /// This shape with `tool` by `op`.
    pub fn boolean(&self, tool: &Shape, op: Boolean) -> Result<Shape> {
        // SAFETY: live handles.
        shape(unsafe {
            match op {
                Boolean::Fuse => raw::occt_fuse(self.0, tool.0),
                Boolean::Cut => raw::occt_cut(self.0, tool.0),
                Boolean::Common => raw::occt_common(self.0, tool.0),
            }
        })
    }

    /// Constant-radius rounds of the edges nearest `points`.
    pub fn fillet_nearest(&self, points: &[[f64; 3]], radius: f64) -> Result<Shape> {
        let flat: Vec<f64> = points.iter().flatten().copied().collect();
        // SAFETY: a live handle and `points.len()` triples of doubles.
        shape(unsafe { raw::occt_fillet_nearest(self.0, flat.as_ptr(), points.len(), radius) })
    }

    /// Whether OCCT's own checker passes the shape.
    pub fn is_valid(&self) -> bool {
        // SAFETY: a live handle.
        unsafe { raw::occt_shape_is_valid(self.0) == 1 }
    }

    /// How many solids the shape holds.
    pub fn solid_count(&self) -> Result<usize> {
        // SAFETY: a live handle.
        let n = unsafe { raw::occt_solid_count(self.0) };
        usize::try_from(n).map_err(|_| last_error())
    }

    /// The volume OCCT measures.
    pub fn volume(&self) -> f64 {
        // SAFETY: a live handle.
        unsafe { raw::occt_volume(self.0) }
    }

    /// The area OCCT measures.
    pub fn area(&self) -> f64 {
        // SAFETY: a live handle.
        unsafe { raw::occt_area(self.0) }
    }

    /// The shape as STEP text, by OCCT's writer.
    pub fn to_step(&self) -> Result<String> {
        // SAFETY: a live handle.
        let text = buffer(unsafe { raw::occt_step_write(self.0) })?;
        String::from_utf8(text).map_err(|_| Error("the STEP text is not UTF-8".into()))
    }

    /// The shape of STEP text, by OCCT's reader.
    pub fn from_step(text: &str) -> Result<Shape> {
        // SAFETY: `text.len()` readable bytes.
        shape(unsafe { raw::occt_step_read(text.as_ptr(), text.len()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_module_is_the_abi_these_bindings_declare() {
        assert_eq!(abi_version(), ABI_VERSION);
        assert!(!library_version().is_empty());
    }

    #[test]
    fn a_box_cut_by_a_box_measures_what_is_left() {
        let a = Shape::make_box([0.0; 3], [2.0, 2.0, 2.0]).unwrap();
        let b = Shape::make_box([1.0, 1.0, 1.0], [2.0, 2.0, 2.0]).unwrap();
        let cut = a.boolean(&b, Boolean::Cut).unwrap();
        assert!((cut.volume() - 7.0).abs() < 1e-9);
        assert_eq!(cut.solid_count().unwrap(), 1);
    }

    #[test]
    fn a_shape_goes_through_step_and_back() {
        let s = Shape::make_sphere([1.0, 2.0, 3.0], 1.5).unwrap();
        let back = Shape::from_step(&s.to_step().unwrap()).unwrap();
        assert!((back.volume() - s.volume()).abs() < 1e-9);
    }

    #[test]
    fn a_refusal_comes_back_in_words() {
        let e = Shape::make_torus([0.0; 3], [0.0, 0.0, 1.0], 1.0, 2.0).unwrap_err();
        assert!(e.0.contains("torus"), "{e}");
    }
}
