use solidcore_record::Vec3;
use solidcraft_geom::Aabb3;
use solidcraft_geom::prelude::*;

use crate::grid::{Grid3, Occupancy};
use crate::tag::FaceTags;
use crate::test_shapes::block_at;

/// `block_at` numbers a box's sides 0 to 5 in the order −x, +x, −y, +y, −z, +z.
const MINUS_X: u32 = 0;
const PLUS_X: u32 = 1;
const MINUS_Y: u32 = 2;
const PLUS_Z: u32 = 5;

fn slab() -> (Occupancy, Vec<solidcraft_geom::Mesh>) {
    let b = Aabb3 { min: Vec3::new(0., 0., 0.), max: Vec3::new(100., 20., 20.) };
    let grid = Grid3::over(b, 20, 1).expect("lattice");
    let mesh = block_at([0., 0., 0.], [100., 20., 20.]);
    let occ = Occupancy::of(grid, &[&mesh], 1).expect("occupancy");
    (occ, vec![mesh])
}

#[test]
fn a_blocks_surface_lands_on_the_blocks_own_six_faces() {
    let (occ, meshes) = slab();
    let refs: Vec<&solidcraft_geom::Mesh> = meshes.iter().collect();
    let tags = FaceTags::of(&occ, &refs);
    assert_eq!(tags.unclaimed(), 0, "every face of the surface belongs to a face of the model");

    let patches = tags.patches();
    assert_eq!(patches.len(), 6, "{:?}", patches.iter().map(|p| (p.part, p.face, p.faces.len())).collect::<Vec<_>>());
    for p in &patches {
        assert_eq!(p.part, 0);
        let want = match p.face {
            // The two ends are 4 by 4 cells; the four long sides are 20 by 4.
            MINUS_X | PLUS_X => 16,
            _ => 80,
        };
        assert_eq!(p.faces.len(), want, "face {} has {} cells", p.face, p.faces.len());
    }
    // Areas add up to the block's own surface area, because every face is square to an axis.
    let total: f64 = patches.iter().map(|p| p.area_m2(&occ)).sum();
    let truth = 2.0 * 0.02 * 0.02 + 4.0 * 0.1 * 0.02;
    assert!((total - truth).abs() < 1e-15, "{total} vs {truth}");
}

#[test]
fn a_tagged_face_points_the_way_that_face_points() {
    let (occ, meshes) = slab();
    let refs: Vec<&solidcraft_geom::Mesh> = meshes.iter().collect();
    let tags = FaceTags::of(&occ, &refs);
    for (i, face) in tags.faces().iter().enumerate() {
        let Some((_, model)) = tags.tag_of(i) else { panic!("an untagged face") };
        // The model face a cell face was matched to has to be the one it looks at.
        let want = match (face.axis(), face.positive()) {
            (0, false) => MINUS_X,
            (0, true) => PLUS_X,
            (1, false) => MINUS_Y,
            (1, true) => 3,
            (2, false) => 4,
            (2, true) => PLUS_Z,
            other => panic!("a face along {other:?}, and a lattice has three axes"),
        };
        assert_eq!(model, want, "a face looking {:?} was matched to model face {model}", face.outward());
    }
}

#[test]
fn picking_a_face_gives_the_same_patch_as_naming_its_side() {
    let (occ, meshes) = slab();
    let refs: Vec<&solidcraft_geom::Mesh> = meshes.iter().collect();
    let tags = FaceTags::of(&occ, &refs);
    let picked = tags.patch(0, PLUS_Z);
    let mut by_side: Vec<_> = occ.surface().filter(|f| f.axis() == 2 && f.positive()).collect();
    by_side.sort();
    let mut picked = picked;
    picked.sort();
    assert_eq!(picked, by_side, "the top face of a block is the lattice's +z side");
}

#[test]
fn picking_several_faces_gives_their_union() {
    let (occ, meshes) = slab();
    let refs: Vec<&solidcraft_geom::Mesh> = meshes.iter().collect();
    let tags = FaceTags::of(&occ, &refs);
    let both = tags.patch_of(&[(0, MINUS_X), (0, PLUS_X)]);
    assert_eq!(both.len(), 32, "the two ends together");
    // And asking for a face no body has gives nothing rather than everything.
    assert!(tags.patch_of(&[(0, 99)]).is_empty());
    assert!(tags.patch_of(&[(7, 0)]).is_empty());
    assert!(tags.patch_of(&[]).is_empty());
}

#[test]
fn two_bodies_keep_their_own_faces_and_lose_the_one_between_them() {
    let left = block_at([0., 0., 0.], [50., 20., 20.]);
    let right = block_at([50., 0., 0.], [50., 20., 20.]);
    let b = Aabb3 { min: Vec3::new(0., 0., 0.), max: Vec3::new(100., 20., 20.) };
    let grid = Grid3::over(b, 20, 1).expect("lattice");
    let occ = Occupancy::of(grid, &[&left, &right], 1).expect("occupancy");
    let tags = FaceTags::of(&occ, &[&left, &right]);
    assert_eq!(tags.unclaimed(), 0);

    let patches = tags.patches();
    // Each body keeps five faces: the sixth is buried where they meet, and the lattice has no
    // surface there at all, so there is nothing to tag.
    for part in [0u16, 1] {
        let mine: Vec<_> = patches.iter().filter(|p| p.part == part).collect();
        assert_eq!(mine.len(), 5, "body {part}: {:?}", mine.iter().map(|p| p.face).collect::<Vec<_>>());
    }
    // The left body keeps its −x end and loses its +x end; the right body the other way round.
    assert!(patches.iter().any(|p| p.part == 0 && p.face == MINUS_X));
    assert!(!patches.iter().any(|p| p.part == 0 && p.face == PLUS_X), "the buried face is gone");
    assert!(patches.iter().any(|p| p.part == 1 && p.face == PLUS_X));
    assert!(!patches.iter().any(|p| p.part == 1 && p.face == MINUS_X));
    // And no face of one body was claimed by a triangle of the other.
    for (i, face) in tags.faces().iter().enumerate() {
        let Some((part, _)) = tags.tag_of(i) else { continue };
        assert_eq!(occ.part_of(face.cell as usize), Some(part), "a face was given to the wrong body");
    }
}

#[test]
fn a_slanted_face_is_still_found_even_though_the_lattice_is_a_staircase() {
    // A wedge: a box with one corner cut off, so one face runs at forty-five degrees and the
    // cell faces along it point two different ways. Distance does not mind.
    let mut mesh = block_at([0., 0., 0.], [40., 40., 40.]);
    // Replace the +z side with a slope by moving its two far corners down. The box stops being
    // closed in the strict sense, which the inside test tolerates for this purpose: the point
    // of the case is that the tagging follows the triangles, not the axes.
    for v in mesh.positions.iter_mut() {
        if v.z > 39.0 && v.x > 39.0 {
            v.z = 0.0;
        }
    }
    let b = mesh.bounds();
    let grid = Grid3::over(b, 8, 1).expect("lattice");
    let occ = Occupancy::of(grid, &[&mesh], 2).expect("occupancy");
    let tags = FaceTags::of(&occ, &[&mesh]);
    // Every face of the surface found a face of the model: nothing fell through a gap.
    assert_eq!(tags.unclaimed(), 0, "{} faces of the surface found nothing", tags.unclaimed());
    // And the sloped face claims cell faces pointing more than one way, which is exactly what
    // a ray along the cell normal would have got wrong.
    let sloped = tags.patch(0, PLUS_Z);
    let directions: std::collections::BTreeSet<u8> = sloped.iter().map(|f| f.dir).collect();
    assert!(directions.len() > 1, "a slope should claim cell faces pointing several ways: {directions:?}");
}

#[test]
fn tagging_does_not_depend_on_the_order_the_triangles_come_in() {
    let (occ, meshes) = slab();
    let first = FaceTags::of(&occ, &meshes.iter().collect::<Vec<_>>());
    // The same mesh with its triangles reversed must give the same answer.
    let mut reversed = meshes[0].clone();
    let n = reversed.triangles.len();
    reversed.triangles.reverse();
    reversed.tri_face.reverse();
    assert_eq!(reversed.triangles.len(), n);
    let second = FaceTags::of(&occ, &[&reversed]);
    for i in 0..first.faces().len() {
        assert_eq!(first.tag_of(i), second.tag_of(i), "face {i} was tagged differently");
    }
}

#[test]
fn the_centres_of_the_surface_sit_half_a_cell_out_from_their_cells() {
    let (occ, meshes) = slab();
    let refs: Vec<&solidcraft_geom::Mesh> = meshes.iter().collect();
    let tags = FaceTags::of(&occ, &refs);
    let centres = tags.centres(&occ);
    assert_eq!(centres.len(), tags.faces().len());
    let h = occ.grid().spacing();
    for (face, at) in tags.faces().iter().zip(&centres) {
        let cell = occ.grid().centre(face.cell as usize).expect("centre");
        let want = cell + face.outward() * (h / 2.0);
        assert!((*at - want).len() < 1e-12, "{at:?} vs {want:?}");
        // Which, for this block, is a point on the block's own surface.
        let on_surface = [at.x, at.y, at.z].iter().zip([100.0, 20.0, 20.0]).any(|(v, size)| v.abs() < 1e-9 || (v - size).abs() < 1e-9);
        assert!(on_surface, "{at:?} is not on the block");
    }
}

/// A face square to the axes is the one case a lattice holds exactly, so the correction must
/// leave it alone to the last digit.
#[test]
fn a_face_square_to_the_axes_needs_no_correcting() {
    let (occ, meshes) = slab();
    let refs: Vec<&solidcraft_geom::Mesh> = meshes.iter().collect();
    let tags = FaceTags::of(&occ, &refs);
    let scale = tags.area_scale(&occ, &refs);
    assert_eq!(scale.len(), tags.faces().len());
    for (face, s) in tags.faces().iter().zip(&scale) {
        assert!((*s - 1.0).abs() < 1e-5, "face {face:?} on a box was scaled by {s}");
    }
}

/// A cylinder's curved side is the case that shows what the correction is for: the staircase
/// standing in for it has a perimeter of `8r` where the circle's is `2πr`, so its area is
/// `4/π` -- 27 per cent -- too large, and stays that way however fine the lattice is cut.
#[test]
fn a_cylinder_carries_the_area_a_cylinder_has() {
    let (radius, z0, z1) = (10.0, 0.0, 20.0);
    let mesh = crate::test_shapes::cylinder(radius, z0, z1, 256);
    let refs = [&mesh];
    let side = std::f64::consts::TAU * radius * (z1 - z0);
    let cap = std::f64::consts::PI * radius * radius;
    for span in [24usize, 48] {
        let grid = Grid3::over(mesh.bounds(), span, 0).expect("a lattice over the cylinder");
        let occ = Occupancy::of(grid, &refs, 4).expect("the cylinder on the lattice");
        let tags = FaceTags::of(&occ, &refs);
        let scale = tags.area_scale(&occ, &refs);
        let cell = occ.grid().face_area_m2() * 1e6;

        let mut staircase = [0.0f64; 3];
        let mut corrected = [0.0f64; 3];
        for i in 0..tags.faces().len() {
            let Some((_, which)) = tags.tag_of(i) else { continue };
            let Some(slot) = staircase.get_mut(which as usize) else { continue };
            *slot += cell;
            let s = scale.get(i).copied().unwrap_or(1.0) as f64;
            if let Some(slot) = corrected.get_mut(which as usize) {
                *slot += cell * s;
            }
        }

        // The staircase really is about 4/π too large on the side, and that is the whole point.
        let over = staircase[0] / side;
        assert!(over > 1.2 && over < 1.33, "span {span}: the staircase side is {over} times the cylinder's");
        // Corrected, the side comes out the area the cylinder has.
        let error = (corrected[0] - side).abs() / side;
        assert!(error < 1e-3, "span {span}: the side came out {} against {side} mm2", corrected[0]);
        // The caps are square to the axes, so the staircase has them nearly right already, and
        // the correction keeps them right.
        for (which, got) in corrected.iter().enumerate().skip(1) {
            let error = (got - cap).abs() / cap;
            assert!(error < 1e-3, "span {span}: cap {which} came out {got} against {cap} mm2");
        }
    }
}

/// And a flat face at 45° is out by √2, which no finer lattice ever fixes either.
#[test]
fn a_slanted_face_carries_the_area_it_has() {
    let leg = 20.0;
    let width = 20.0;
    let mesh = crate::test_shapes::wedge(leg, width);
    let refs = [&mesh];
    let slant = leg * 2.0f64.sqrt() * width;
    let grid = Grid3::over(mesh.bounds(), 32, 0).expect("a lattice over the wedge");
    let occ = Occupancy::of(grid, &refs, 4).expect("the wedge on the lattice");
    let tags = FaceTags::of(&occ, &refs);
    let scale = tags.area_scale(&occ, &refs);
    let cell = occ.grid().face_area_m2() * 1e6;

    let mut staircase = 0.0;
    let mut corrected = 0.0;
    for (i, _) in tags.faces().iter().enumerate() {
        let Some((_, which)) = tags.tag_of(i) else { continue };
        if which != 0 {
            continue;
        }
        staircase += cell;
        corrected += cell * scale.get(i).copied().unwrap_or(1.0) as f64;
    }
    let over = staircase / slant;
    assert!(over > 1.3 && over < 1.5, "the staircase slant is {over} times the real one");
    let error = (corrected - slant).abs() / slant;
    assert!(error < 1e-3, "the slant came out {corrected} against {slant} mm2");
}
