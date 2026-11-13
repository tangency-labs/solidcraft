//! Each bounded region's outer shell, found from the topology alone.
//!
//! Not upstream in SMLib. SMLib's convention is that a region's first shell is its outer shell
//! (`SmBrep::AssertValid` test 12), but its own operations do not always keep it: a union that
//! seals a void can leave the void's shell first in the material region's list. An exporter or
//! record that needs the outer shell takes it from [`outer_shells`] instead of from the list
//! order.
//!
//! The region tree is walked from the infinite region through the shells' mates (each face use's
//! mate, the other side of its face). A region's outer shell is the shell mated to the region it
//! was first reached from: the region enclosing it. Its other shells bound regions nested inside
//! it. The walk is breadth first over regions in list order, shells in list order and face uses
//! in list order, so the result is deterministic and does not depend on the shells' order within
//! a region. It reads the B-rep only (no geometry, no kernel marks).

use std::collections::BTreeMap;

use crate::error::{Result, SolidError};
use crate::topology::{Brep, RegionId, ShellId};

const OP: &str = "topology::brep::outer_shells";

/// The shells on the other side of `shell`'s faces, distinct, in first-seen face-use order
/// (`SmShell::GetMatedShells`'s order, without its context marks).
fn mated_shells(b: &Brep, shell: ShellId) -> Result<Vec<ShellId>> {
    let mut out: Vec<ShellId> = Vec::new();
    for faceuse in b.shell(shell)?.get_faceuses()? {
        let mate = b
            .faceuse(faceuse)?
            .mate
            .ok_or(SolidError::invalid_input(OP, "a face use without its mate"))?;
        let other = b.faceuse(mate)?.shell.ok_or(SolidError::invalid_input(
            OP,
            "a face use without its shell",
        ))?;
        if !out.contains(&other) {
            out.push(other);
        }
    }
    Ok(out)
}

/// Each bounded region's outer shell, by region: the shell mated to the region enclosing it.
/// The infinite region has none, and neither has a region no shell walk from the infinite
/// region reaches (a B-rep without an infinite region gives an empty map).
///
/// # Errors
///
/// A dangling id, or a face use without its mate or shell.
pub fn outer_shells(b: &Brep) -> Result<BTreeMap<RegionId, ShellId>> {
    let mut outer = BTreeMap::new();
    let Some(infinite) = b.infinite_region() else {
        return Ok(outer);
    };
    let mut reached = vec![infinite];
    let mut next = 0;
    while let Some(&region) = reached.get(next) {
        next += 1;
        for &shell in &b.region(region)?.shells {
            for mate in mated_shells(b, shell)? {
                let Some(beyond) = b.shell(mate)?.region else {
                    continue;
                };
                if !reached.contains(&beyond) {
                    reached.push(beyond);
                    outer.insert(beyond, mate);
                }
            }
        }
    }
    Ok(outer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boolean::api_brep::boolean;
    use crate::geom::a_object::BooleanOperation;
    use crate::kernel::Kernel;
    use crate::math::Point3;
    use crate::primitive::api_primitives::create_box;
    use crate::topology::brep::p1::regions;

    /// The cup and lid of WR-265 on the default kernel, which leaves the cavity's shell first
    /// in the solid region's list: for either operand order, the solid's outer shell is the one
    /// mated to the infinite region, not its list head, and the cavity's outer shell is the
    /// solid's other shell.
    #[test]
    fn the_outer_shell_of_a_solid_whose_void_is_listed_first() {
        for lid_first in [false, true] {
            let mut k = Kernel::default();
            let cube = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 100.0, 100.0, 100.0).unwrap();
            let hollow =
                create_box(&mut k, Point3::new(10.0, 10.0, 10.0), 80.0, 80.0, 100.0).unwrap();
            let lid = create_box(&mut k, Point3::new(0.0, 0.0, 90.0), 100.0, 100.0, 10.0).unwrap();
            let cup = boolean(&mut k, cube, hollow, BooleanOperation::Difference)
                .unwrap()
                .result;
            let (a, b) = if lid_first { (lid, cup) } else { (cup, lid) };
            let r = boolean(&mut k, a, b, BooleanOperation::Union)
                .unwrap()
                .result;
            let infinite = r.infinite_region().unwrap();
            let all = regions(&r, None).unwrap();
            let solid = *all
                .iter()
                .find(|&&g| r.region(g).unwrap().is_void() == 0)
                .unwrap();
            let cavity = *all
                .iter()
                .find(|&&g| g != infinite && r.region(g).unwrap().is_void() != 0)
                .unwrap();
            let shells = r.region(solid).unwrap().shells.clone();
            assert_eq!(shells.len(), 2, "lid first: {lid_first}");
            let outer = outer_shells(&r).unwrap();
            assert_eq!(outer.len(), 2, "lid first: {lid_first}");
            assert!(!outer.contains_key(&infinite));
            // the solid's outer shell: its list's second, the one facing the infinite region
            assert_eq!(
                outer.get(&solid),
                Some(&shells[1]),
                "lid first: {lid_first}"
            );
            for m in mated_shells(&r, shells[1]).unwrap() {
                assert_eq!(r.shell(m).unwrap().region, Some(infinite));
            }
            // the cavity's: the shell mated to the solid's first shell
            let [cavity_shell] = mated_shells(&r, shells[0]).unwrap()[..] else {
                panic!("one shell beyond the cavity's faces")
            };
            assert_eq!(outer.get(&cavity), Some(&cavity_shell));
            assert_eq!(r.shell(cavity_shell).unwrap().region, Some(cavity));
        }
    }

    /// A box: its solid's one shell.
    #[test]
    fn the_outer_shell_of_a_box() {
        let mut k = Kernel::default();
        let r = create_box(&mut k, Point3::new(1.0, 2.0, 3.0), 4.0, 5.0, 6.0).unwrap();
        let infinite = r.infinite_region().unwrap();
        let solid = *regions(&r, None)
            .unwrap()
            .iter()
            .find(|&&g| g != infinite)
            .unwrap();
        let outer = outer_shells(&r).unwrap();
        assert_eq!(outer.len(), 1);
        assert_eq!(outer.get(&solid), r.region(solid).unwrap().shells.first());
    }
}
