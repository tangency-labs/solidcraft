//! SMLib's own validity check: `SmBrep::ValidatePointers`, then `AssertValid` over the whole
//! topology graph. Tests run it on every SMLib result and every import.

use crate::{Brep, Result, core};

/// How many of SMLib's tests to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckLevel {
    /// The fewest: topology and pointers, no geometry.
    Basic = 0,
    /// Adds the geometry checks (gaps between vertices, edges and faces against tolerances).
    Geometry = 1,
    /// Every test, the slow ones too.
    All = 2,
}

/// What SMLib's check found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckReport {
    /// Failed tests; 0 means SMLib finds the body valid.
    pub failures: usize,
    /// One line per failed test, such as
    /// `SmBrep::Test #82 Tolerance: Vertex tolerance relative to brep tolerance (offset=2, tolerance=0.01) [owner SmVertex]`.
    pub text: String,
}

impl CheckReport {
    pub fn is_valid(&self) -> bool {
        self.failures == 0
    }

    /// The number of each failed test, line by line (`SmBrep::Test #82 …` is 82); a line without
    /// one is left out.
    pub fn failed_tests(&self) -> Vec<u32> {
        self.text
            .lines()
            .filter_map(|l| {
                let rest = l.split_once("Test #")?.1;
                let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
                digits.parse().ok()
            })
            .collect()
    }
}

impl std::fmt::Display for CheckReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_valid() { f.write_str("valid") } else { write!(f, "{} failed tests:\n{}", self.failures, self.text) }
    }
}

impl Brep {
    /// Runs SMLib's validity check at `level`. A body that fails it is still `Ok`, with the
    /// failures in the report; `Err` means the check could not run.
    pub fn check(&self, level: CheckLevel) -> Result<CheckReport> {
        core::check(self.core(), level)
    }

    /// [`Brep::check`] of a body nobody else reads, which it takes: the token over this thread's
    /// work ([`crate::stop`]) stops it ([`crate::Error::Cancelled`]). A check left part way may
    /// leave the body's caches half-built, so it runs only on a body it consumes.
    pub fn check_owned(self, level: CheckLevel) -> Result<CheckReport> {
        core::check_with(self.core(), level, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stopped token stops the check of a body the check owns, and never the check of a body
    /// others read (solidcore#3247): stopped part way, it could leave that body's caches half-built.
    #[test]
    fn a_stopped_token_stops_an_owned_check_and_never_a_shared_one() {
        let b = Brep::make_box([0.0; 3], [10.0, 20.0, 30.0]).unwrap();
        let mine = b.try_clone().unwrap();
        let stop = crate::stop::Stop::new();
        stop.stop();
        let shared = stop.within(|| b.check(CheckLevel::All));
        assert!(shared.is_ok_and(|r| r.is_valid()));
        let owned = stop.within(|| mine.check_owned(CheckLevel::All));
        assert!(matches!(owned, Err(crate::Error::Cancelled("check"))), "{owned:?}");
        let mine = b.try_clone().unwrap();
        assert!(mine.check_owned(CheckLevel::All).is_ok_and(|r| r.is_valid()), "no token, no stop");
    }

    #[test]
    fn a_box_is_valid_at_every_level() {
        let b = Brep::make_box([1.0, 2.0, 3.0], [10.0, 20.0, 30.0]).unwrap();
        for level in [CheckLevel::Basic, CheckLevel::Geometry, CheckLevel::All] {
            let r = b.check(level).unwrap();
            assert!(r.is_valid(), "{level:?}: {r}");
            assert_eq!(r.text, "");
        }
    }

    #[test]
    fn every_primitive_is_valid_at_every_level() {
        let o = [1.0, 2.0, 3.0];
        let bodies = [
            ("cylinder", Brep::make_cylinder(o, 15.0, 50.0).unwrap()),
            ("sphere", Brep::make_sphere(o, 12.0).unwrap()),
            ("frustum", Brep::make_cone(o, 10.0, 4.0, 20.0).unwrap()),
            ("cone", Brep::make_cone(o, 10.0, 0.0, 20.0).unwrap()),
            ("torus", Brep::make_torus(o, 22.0, 4.0).unwrap()),
        ];
        for (name, b) in &bodies {
            for level in [CheckLevel::Basic, CheckLevel::Geometry, CheckLevel::All] {
                let r = b.check(level).unwrap();
                assert!(r.is_valid(), "{name}, {level:?}: {r}");
            }
        }
    }

    #[test]
    fn a_copy_and_a_native_file_round_trip_stay_valid() {
        let b = Brep::make_box([0.0; 3], [2.0, 3.0, 4.0]).unwrap();
        assert!(b.try_clone().unwrap().check(CheckLevel::All).unwrap().is_valid());
        let dir = std::env::temp_dir().join(format!("solidcraft-smlib-check-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("box.smb");
        b.write(&path, false).unwrap();
        let back = Brep::read(&path, false).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let r = back.check(CheckLevel::All).unwrap();
        assert!(r.is_valid(), "{r}");
    }
}
