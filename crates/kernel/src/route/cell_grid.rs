//! The router's cell grids ([`solidcraft_kernel_api::CellGrid`]).

use solidcore_record::cell_grid::{CellGridPlan, CellGridSpec, GridCell, SketchCell};
use solidcore_record::cell_grid_wall::{CylinderWall, WallCellPlan, WallDepth};
use solidcore_record::{Loop2, Region2, Vec2, Vec3};
use solidcraft_kernel_api::{CellGrid, CellGridLimits, OpKind};

use super::{Which, which};
use crate::backend::Solidcore;
use crate::{Body, Result};

/// The ranges a cell grid's spec is taken in: the kernel's. This build holds solidcore only, so
/// they are solidcore's, known when it is compiled.
pub const CELL_GRID_LIMITS: CellGridLimits = <Solidcore as CellGrid>::LIMITS;

/// The grid a new cell grid starts from.
pub fn default_cell_grid() -> CellGridSpec {
    match which(OpKind::CellGrid) {
        Which::Solidcore => Solidcore.default_cell_grid(),
    }
}

/// The cells of the grid `spec` over `field` (nested closed loops: a field, a keep-out inside it,
/// and so on). A refusal is [`crate::KernelError::Invalid`] with the kernel's words.
pub fn plan_cell_grid(field: &[Loop2], spec: &CellGridSpec) -> Result<CellGridPlan> {
    match which(OpKind::CellGrid) {
        Which::Solidcore => Solidcore.plan_cell_grid(field, spec),
    }
}

/// The cells of a grid of the sketched cell `cell` over `field`, nested as for [`plan_cell_grid`].
/// A refusal is [`crate::KernelError::Invalid`] with the kernel's words.
pub fn plan_sketch_cell_grid(field: &[Loop2], cell: &SketchCell, spec: &CellGridSpec) -> Result<CellGridPlan> {
    match which(OpKind::CellGrid) {
        Which::Solidcore => Solidcore.plan_sketch_cell_grid(field, cell, spec),
    }
}

/// The opening of `cell` as regions. A refusal is [`crate::KernelError::Invalid`] with the
/// kernel's words.
pub fn cell_regions(cell: &GridCell) -> Result<Vec<Region2>> {
    match which(OpKind::CellGrid) {
        Which::Solidcore => Solidcore.cell_regions(cell),
    }
}

/// The whole-turn cylinder wall of face `face` of `body`, outside or bore. A mesh body, a face
/// that is not such a wall and a face index out of range are [`crate::KernelError::Invalid`] with
/// the kernel's words.
pub fn cylinder_wall(body: &Body, face: usize) -> Result<CylinderWall> {
    body.require_brep("a cell grid on a wall")?;
    match which(OpKind::CellGrid) {
        Which::Solidcore => Solidcore.cylinder_wall(body.record(), face),
    }
}

/// The cells of the grid `spec` round `wall`, cut `depth` into it. A refusal is
/// [`crate::KernelError::Invalid`] with the kernel's words.
pub fn plan_wall_cell_grid(wall: &CylinderWall, spec: &CellGridSpec, depth: WallDepth) -> Result<WallCellPlan> {
    match which(OpKind::CellGrid) {
        Which::Solidcore => Solidcore.plan_wall_cell_grid(wall, spec, depth),
    }
}

/// The point `p` as unrolled coordinates of `wall`. A refusal is [`crate::KernelError::Invalid`].
pub fn unroll_wall_point(wall: &CylinderWall, p: Vec3) -> Result<Vec2> {
    match which(OpKind::CellGrid) {
        Which::Solidcore => Solidcore.unroll_wall_point(wall, p),
    }
}

/// The anchor a cell grid on `wall` gets when none is given. A refusal is
/// [`crate::KernelError::Invalid`].
pub fn default_wall_anchor(wall: &CylinderWall, spec: &CellGridSpec) -> Result<Vec2> {
    match which(OpKind::CellGrid) {
        Which::Solidcore => Solidcore.default_wall_anchor(wall, spec),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::Vec2;

    fn square(side: f64) -> Loop2 {
        let p = |x, y| Vec2::new(x, y);
        Loop2::polygon(&[p(0.0, 0.0), p(side, 0.0), p(side, side), p(0.0, side)])
    }

    #[test]
    fn the_routed_grid_is_solidcores() {
        use solidcore_ops::cell_grid as ops;
        assert_eq!(CELL_GRID_LIMITS, CellGridLimits { min_size: ops::MIN_SIZE, max_sides: ops::MAX_SIDES, max_snap_inset: ops::MAX_SNAP_INSET });
        let spec = default_cell_grid();
        assert_eq!(spec, ops::default_spec());
        let field = [square(40.0)];
        let plan = plan_cell_grid(&field, &spec).unwrap();
        assert_eq!(plan, ops::plan_cell_grid(&field, &spec).unwrap());
        let cell = plan.cells.first().unwrap();
        assert_eq!(cell_regions(cell).unwrap(), ops::cell_regions(cell).unwrap());
        // a refusal carries solidcore's words whole
        let bad = CellGridSpec { size: f64::NAN, ..spec };
        let words = ops::plan_cell_grid(&field, &bad).unwrap_err().to_string();
        assert_eq!(plan_cell_grid(&field, &bad).unwrap_err(), crate::KernelError::Invalid(words));
    }

    #[test]
    fn the_routed_sketched_grid_is_solidcores() {
        use solidcore_ops::cell_grid as ops;
        use solidcore_record::cell_grid::{Clip, SketchCell};
        // a sketched cell has no chords of its own to snap along, so it takes whole cells
        let spec = CellGridSpec { clip: Clip::Whole, ..default_cell_grid() };
        let field = [square(60.0)];
        let cell = SketchCell { profile: square(8.0), origin: None, pitch_x: 12.0, pitch_y: 12.0, stagger: 0.5, cell_angle: 0.0, flip: false };
        let plan = plan_sketch_cell_grid(&field, &cell, &spec).unwrap();
        assert!(!plan.cells.is_empty());
        assert_eq!(plan, ops::plan_sketch_cell_grid(&field, &cell, &spec).unwrap());
        // a refusal carries solidcore's words whole
        let bad = SketchCell { pitch_x: f64::NAN, ..cell };
        let words = ops::plan_sketch_cell_grid(&field, &bad, &spec).unwrap_err().to_string();
        assert_eq!(plan_sketch_cell_grid(&field, &bad, &spec).unwrap_err(), crate::KernelError::Invalid(words));
    }

    #[test]
    fn the_routed_wall_plan_is_solidcores() {
        use solidcore_ops::cell_grid_wall as ops;
        use solidcore_record::cell_grid::{CellShape, Clip};
        // a tube of outer radius 30 and bore 27, 40 long, built as a revolved ring
        let ring = solidcore_record::Region2 {
            outer: Loop2::polygon(&[Vec2::new(27.0, 0.0), Vec2::new(30.0, 0.0), Vec2::new(30.0, 40.0), Vec2::new(27.0, 40.0)]),
            holes: vec![],
        };
        let tube = crate::revolve(&solidcore_record::Plane::YZ, &[ring], Vec2::ZERO, Vec2::Y, std::f64::consts::TAU).unwrap().pop().unwrap();
        let outside =
            (0..tube.record().faces.len()).find_map(|f| cylinder_wall(&tube, f).ok().filter(|w| w.convex)).expect("the tube has an outside wall");
        assert_eq!(outside.radius, 30.0);
        let face = (0..tube.record().faces.len()).find(|f| cylinder_wall(&tube, *f).ok() == Some(outside)).unwrap();
        assert_eq!(outside, ops::cylinder_wall(tube.record(), face, tube.record().tolerance).unwrap());
        // 54 cells of size s and web 2 close round 2 pi 30
        let pitch = std::f64::consts::TAU * 30.0 / 18.0;
        let spec = CellGridSpec { shape: CellShape::Square, size: pitch - 2.0, web: 2.0, clip: Clip::Whole, ..default_cell_grid() };
        let anchor = default_wall_anchor(&outside, &spec).unwrap();
        let [ax, ay] = ops::default_wall_anchor(&outside, &spec).unwrap();
        assert_eq!((anchor.x, anchor.y), (ax, ay));
        let spec = CellGridSpec { anchor, ..spec };
        let plan = plan_wall_cell_grid(&outside, &spec, WallDepth::Through).unwrap();
        assert_eq!(plan, ops::plan_wall_cell_grid(&outside, &spec, WallDepth::Through).unwrap());
        assert!(!plan.sectors.is_empty());
        let p = Vec3::new(30.0, 0.0, 20.0);
        let [ux, uy] = ops::unroll_wall_point(&outside, [30.0, 0.0, 20.0]).unwrap();
        let u = unroll_wall_point(&outside, p).unwrap();
        assert_eq!((u.x, u.y), (ux, uy));
        // a refusal carries solidcore's words whole
        let bad = CellGridSpec { size: f64::NAN, ..spec };
        let words = ops::plan_wall_cell_grid(&outside, &bad, WallDepth::Through).unwrap_err().to_string();
        assert_eq!(plan_wall_cell_grid(&outside, &bad, WallDepth::Through).unwrap_err(), crate::KernelError::Invalid(words));
    }

    #[test]
    fn a_face_that_is_no_cylinder_wall_is_refused_in_words() {
        use solidcore_record::Region2;
        let b = crate::box_solid(Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 10.0)).unwrap();
        assert!(matches!(cylinder_wall(&b, 0), Err(crate::KernelError::Invalid(w)) if !w.is_empty()));
        // a face index out of range is a refusal, not a panic
        assert!(matches!(cylinder_wall(&b, 9999), Err(crate::KernelError::Invalid(_))));
        let ball = crate::sphere(Vec3::new(0.0, 0.0, 0.0), 5.0).unwrap();
        assert!(matches!(cylinder_wall(&ball, 0), Err(crate::KernelError::Invalid(w)) if !w.is_empty()));
        let outline =
            Region2 { outer: Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(8.0, 0.0), Vec2::new(5.0, 20.0), Vec2::new(0.0, 20.0)]), holes: vec![] };
        let cone = crate::revolve(&solidcore_record::Plane::YZ, &[outline], Vec2::ZERO, Vec2::Y, std::f64::consts::TAU).unwrap().pop().unwrap();
        let words: Vec<String> = (0..cone.record().faces.len())
            .filter_map(|f| match cylinder_wall(&cone, f) {
                Err(crate::KernelError::Invalid(w)) => Some(w),
                _ => None,
            })
            .collect();
        assert!(words.iter().any(|w| w.contains("cone")), "{words:?}");
    }

    #[test]
    fn a_mesh_body_has_no_cylinder_wall() {
        let c = |i: u32| Vec3::new((i & 1) as f64 * 10.0, ((i >> 1) & 1) as f64 * 10.0, ((i >> 2) & 1) as f64 * 10.0);
        let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
        let (mut pos, mut tris) = (Vec::new(), Vec::new());
        for q in quads {
            for t in [[q[0], q[2], q[1]], [q[0], q[3], q[2]]] {
                let n = pos.len() as u32;
                pos.extend(t.iter().map(|i| c(*i)));
                tris.push([n, n + 1, n + 2]);
            }
        }
        let mesh = crate::mesh_body(&pos, &tris).unwrap();
        let words = match cylinder_wall(&mesh, 0) {
            Err(crate::KernelError::Invalid(w)) => w,
            other => panic!("{other:?}"),
        };
        assert!(words.contains("mesh"), "{words}");
    }
}
