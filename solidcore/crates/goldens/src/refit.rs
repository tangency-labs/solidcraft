//! A test seam at the runners' file reads, between parsing a file and building its
//! topology: [`before_topology`] lets this crate's tests put, in place of each file NURB
//! the read finds to be a sphere, the sphere as upstream's `IsNurbSurfaceSphere` returns
//! it (its NURB made from the fitted circles). Outside `cfg(test)` it does nothing.
//!
//! The WR-493 replay (`refit::replay`) uses it to show that a golden row moved only
//! through the NURB a found sphere keeps: with the fitted NURB put back, the read drops
//! the file's UV trims on it and rebuilds them, as main did before the retirement, and
//! the row is main's row bit for bit.
//!
//! The seams are the readers the runners share ([`read_from_file`]: [`crate::own_reader`],
//! `topology.api_files`, `topology.brep_p7` and `corpus.parity`'s B-rep reads) and
//! `topology.brep_p10`'s `make_topology_from_data` cases. While a mode is installed
//! ([`active`]), `topology.face_p1`'s cache of recent reads is bypassed.

use solidcore::kernel::Kernel;
use solidcore::topology::brep::{p10::TopologyEnv, p7};
use solidcore::topology::brep_data::BrepData;
use solidcore::topology::database_io::FileType;
use solidcore::topology::Brep;
use solidcore::Result as SolidResult;

/// `p7::read_from_file` with the seam between the parse and the topology: the same calls
/// in the same order ([`p7::read_from_data`] is `read_from_file`'s second half).
pub(crate) fn read_from_file(
    k: &mut Kernel,
    b: &mut Brep,
    bytes: Vec<u8>,
    file_type: FileType,
    rebuild_uv_trim_curves: bool,
    env: &mut TopologyEnv<'_>,
) -> SolidResult<()> {
    let mut data = BrepData::read_from_file(k, bytes, file_type)?;
    // (`ReadFromFile` makes the topology with `bAddAnalytics`)
    before_topology(k, &mut data, true);
    p7::read_from_data(k, b, data, rebuild_uv_trim_curves, env)
}

/// Called by every seam with the parsed file and whether the topology it makes adds
/// analytic surfaces, before the topology is made.
pub(crate) fn before_topology(k: &Kernel, data: &mut BrepData, add_analytics: bool) {
    #[cfg(test)]
    state::before_topology(k, data, add_analytics);
    #[cfg(not(test))]
    let _ = (k, data, add_analytics);
}

/// Whether a test has installed a mode: readers that cache a read then read afresh, so
/// the seam sees every read. Always `false` outside `cfg(test)`.
#[cfg(test)]
pub(crate) fn active() -> bool {
    state::active()
}

/// [`active`] outside `cfg(test)`.
#[cfg(not(test))]
pub(crate) fn active() -> bool {
    false
}

#[cfg(test)]
mod replay;

/// Runs `f` with every found sphere given upstream's fitted NURB ([`state::Refit`]), as
/// main built it before the WR-493 retirement; returns its result and how many file NURBs
/// were replaced. The mode is removed even when `f` panics.
#[cfg(test)]
pub(crate) fn with_fitted_spheres<T>(f: impl FnOnce() -> T) -> (T, usize) {
    struct Clear;
    impl Drop for Clear {
        fn drop(&mut self) {
            if std::thread::panicking() {
                state::set(None);
            }
        }
    }
    state::set(Some(state::Refit::default()));
    let clear = Clear;
    let out = f();
    drop(clear);
    let r = state::take().expect("refit mode lost");
    assert!(r.failures.is_empty(), "{:?}", r.failures);
    (out, r.replaced)
}

#[cfg(test)]
pub(crate) mod state {
    use std::cell::RefCell;

    use solidcore::kernel::Kernel;
    use solidcore::surface::cone::Cone;
    use solidcore::surface::plane::Plane;
    use solidcore::surface::sphere::Sphere;
    use solidcore::surface::surface::{Surface, SurfaceKind};
    use solidcore::topology::brep_data::BrepData;

    /// What the seam saw while installed.
    #[derive(Debug, Default)]
    pub(crate) struct Refit {
        /// Reads seen.
        pub reads: u32,
        /// File NURBs replaced by the fitted sphere's, over all reads.
        pub replaced: usize,
        /// Recognition errors and unsupported kernels.
        pub failures: Vec<String>,
    }

    thread_local! {
        static MODE: RefCell<Option<Refit>> = const { RefCell::new(None) };
    }

    /// Installs the refit for the reads that follow on this thread.
    pub(crate) fn set(mode: Option<Refit>) {
        MODE.with(|m| *m.borrow_mut() = mode);
    }

    /// Whether the refit is installed.
    pub(crate) fn active() -> bool {
        MODE.with(|m| m.borrow().is_some())
    }

    /// Removes and returns the installed refit.
    pub(crate) fn take() -> Option<Refit> {
        MODE.with(|m| m.borrow_mut().take())
    }

    /// Each B-spline surface of `data` that `CopyAndAddAnalytics` finds to be a sphere
    /// (its recognisers in their order: the plane, the cone, then the sphere, each at
    /// tolerance scale 1) becomes that sphere with upstream's fitted NURB. The topology
    /// then copies it as a sphere, as `CopyAndAddAnalytics` returned it before the retirement.
    pub(super) fn before_topology(k: &Kernel, data: &mut BrepData, add_analytics: bool) {
        MODE.with(|m| {
            let mut m = m.borrow_mut();
            let Some(r) = m.as_mut() else {
                return;
            };
            r.reads += 1;
            if !add_analytics {
                return;
            }
            if k.fixes.analytic_copy_keeps_parameterization {
                // (that fix may keep an exact copy instead: not replayed here)
                r.failures
                    .push("analytic_copy_keeps_parameterization is on".to_string());
                return;
            }
            for (i, s) in data.surfaces.iter_mut().enumerate() {
                let SurfaceKind::BSpline(srf) = s.object.kind() else {
                    continue;
                };
                let fitted = (|| {
                    if Plane::is_nurb_surface_plane(k, srf, 1.0)?.is_some()
                        || Cone::is_nurb_surface_cone(k, srf, 1.0)?.is_some()
                    {
                        return Ok(None);
                    }
                    Sphere::fitted_nurb_surface_sphere(k, srf, 1.0)
                })();
                match fitted {
                    Ok(Some(sphere)) => {
                        s.object = Surface::from(sphere);
                        r.replaced += 1;
                    }
                    Ok(None) => {}
                    Err(e) => r.failures.push(format!("surface {i}: {e}")),
                }
            }
        });
    }
}
