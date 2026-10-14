//! Uniform voxel grids for fast ray casting: elements are registered in every voxel
//! their bounding box touches, and a 3D-DDA walk visits the voxels along a ray and
//! reports the elements whose boxes the ray crosses, sorted by entry parameter.
//!
//! Upstream: `source/SMLib/src/SmGrid.cpp`, `source/SMLib/inc/SmGrid.h`.
//!
//! | Rust                                   | Upstream                                         |
//! |----------------------------------------|--------------------------------------------------|
//! | [`Grid`], [`Grid::new`]                | `SmGrid`, `SmGrid::SmGrid`                       |
//! | `Drop` (implicit)                      | `SmGrid::~SmGrid`                                |
//! | [`Grid::voxel`]                        | `SmGrid::GetVoxel`                               |
//! | [`Grid::voxel_bbox`]                   | `SmGrid::GetVoxelBBox`                           |
//! | [`Grid::voxel_address`]                | `SmGrid::GetVoxelAddress`                        |
//! | [`Grid::find_voxels_in_box`]           | `SmGrid::FindVoxelsInBox`                        |
//! | [`Grid::add_element`], [`ElementSource`] | `SmGrid::AddElement`, `SmGridElementType`      |
//! | [`Grid::add_hierarchy_element`]        | `SmGrid::AddHierarchyElement`                    |
//! | [`Grid::expand_hierarchy`]             | `SmSurfaceHierarchyElement::ExpandHierarchy`     |
//! | [`Grid::setup_3ddda`]                  | `SmGrid::Setup3DDDA`                             |
//! | `AxisStep` (private)                   | one axis of `SmGrid::Setup3DDDA`                 |
//! | [`Grid::step_3ddda`], [`StepHits`]     | `SmGrid::Step3DDDA`                              |
//! | `insertion_sort` (private)             | `sm_InsertionSort`                               |
//! | [`Dda`]                                | the 3DDDA scratch members of `SmGrid`            |
//! | [`Voxel`], `Default`                   | `SmVoxel`, `SmVoxel::Init`                       |
//! | [`GridElement`]                        | `SmGridElement`                                  |
//! | [`GridElement::type_code`]             | `SmGridElement::GetType`                         |
//! | [`GridItem`]                           | the derived `SmGridElement` classes and their `GetElement` |
//! | `GridElement::polygon` (private)       | `SmPolygonGridElement::Init`                     |
//! | `GridElement::hierarchy` (private)     | `SmHierarchyGridElement::Init`                   |
//! | `GridElement::user` (private)          | `SmUserGridElement::Init`                        |
//! | [`SurfaceGridElement`]                 | `SmSurfaceGridElement` (`Init`: see deviations)  |
//! | [`HierarchyElement::new`]              | `SmHierarchyElement::SmHierarchyElement`         |
//! | [`HierarchyElement::new_surface`]      | `SmSurfaceHierarchyElement::SmSurfaceHierarchyElement` |
//! | `Drop` (implicit)                      | `~SmHierarchyElement`, `~SmSurfaceHierarchyElement` |
//!
//! Deviations:
//! - Pointers become arena indices. The grid owns its elements ([`ElementId`], in
//!   creation order; upstream's `SmMemBlockMgr` pools) and its hierarchy elements
//!   ([`HierarchyId`], in insertion order; upstream `m_pHierarchyElements`). Voxels
//!   are stored flat in `x`-major order and addressed by `[ix, iy, iz]`. Objects are a
//!   caller-chosen `Copy` handle `O` (upstream `void*` user data and `SmSurface*`);
//!   polygon elements hold the [`NodeId`] of their `SmBrepCache` tree node.
//! - `SmGrid(bbox, lSize)` rejects a zero voxel count (upstream builds a grid its
//!   destructor then reads out of bounds) and a voxel count whose total overflows.
//! - `AddElement`'s `SM_GE_HIERARCHY` type (a null-pointer crash upstream) cannot be
//!   expressed: [`ElementSource`] has no such variant.
//! - Surface elements need the surface subdivision caches, pseudo boxes and polar
//!   boxes of units not ported yet: `SmSurfaceGridElement::Init` (reached through
//!   [`Grid::add_element`] with [`ElementSource::Surface`]) and
//!   `SmSurfaceHierarchyElement::ExpandHierarchy` return [`SolidError::Unported`]
//!   (see the `UNPORTED` markers); [`SurfaceGridElement`] has no values yet.
//!   `ExpandHierarchy` on a plain [`HierarchyElement::new`] is upstream's base-class
//!   "pure virtual" body: `SM_ERR`.
//! - `GetVoxelAddress` truncates with Rust's saturating `as` cast, so a NaN
//!   coordinate (a grid with a zero-size axis) gives index 0 instead of undefined
//!   behaviour. Its status (always `SM_SUCCESS`) is dropped.
//! - `FindVoxelsInBox` returns voxel addresses instead of `SmVoxel*`; upstream also
//!   computes a tolerance-expanded copy of the box that it never uses, which is
//!   omitted.
//! - `Step3DDDA` returns its output lists ([`StepHits`]); its unused
//!   `lCurrentVoxel` argument is dropped. When the current voxel address is out of
//!   range (no successful [`Grid::setup_3ddda`] yet, or a step after the ray left the
//!   grid; undefined behaviour upstream) it fails with
//!   [`SolidError::InvalidInput`].
//! - The 3DDDA step/exit increments of an axis the ray does not move along keep
//!   their previous values (zero for a new grid); upstream leaves them
//!   uninitialised. Neither is ever read.
//! - `SmHierarchyGridElement`'s sphere centre starts as [`Vec3::UNDEF`] and its box as
//!   [`Extent3d::new`] (upstream's default constructors).
//! - `GetMemoryUsed`, the drawing methods and `SmVoxel::m_pGrid` (a back pointer)
//!   have no counterpart; `SM_ASSERT`, `SM_DBG_WARN` and the `SM_DEBUG_CODE`
//!   drawing are not ported.

use crate::error::{Result, SolidError};
use crate::geom::extent3d::{Extent3d, LineHit};
use crate::geom::tree::NodeId;
use crate::math::{Point3, Vec3, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQRT};

/// `SmGridElement_TYPE` (`CONT_BASE_TYPE + 155`).
pub const GRID_ELEMENT_TYPE: i64 = 12155;
/// `SmSurfaceGridElement_TYPE` (`CONT_BASE_TYPE + 159`).
pub const SURFACE_GRID_ELEMENT_TYPE: i64 = 12159;
/// `SmPolygonGridElement_TYPE` (`CONT_BASE_TYPE + 213`).
pub const POLYGON_GRID_ELEMENT_TYPE: i64 = 12213;
/// `SmHierarchyGridElement_TYPE` (`CONT_BASE_TYPE + 217`).
pub const HIERARCHY_GRID_ELEMENT_TYPE: i64 = 12217;
/// `SmUserGridElement_TYPE` (`CONT_BASE_TYPE + 221`).
pub const USER_GRID_ELEMENT_TYPE: i64 = 12221;

/// Index of a grid element in [`Grid::elements`] (upstream `SmGridElement*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ElementId(pub usize);

/// Index of a hierarchy element in [`Grid::hierarchy_elements`] (upstream
/// `SmHierarchyElement*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HierarchyId(pub usize);

/// Voxel indices `[ix, iy, iz]` (upstream `long lVoxelAddress[3]`).
pub type VoxelAddress = [usize; 3];

/// The cached data of one surface subdivision leaf (`SmSurfaceGridElement`).
///
/// Building one needs `SmBezierPatch`, `SmPseudoBox` and `SmPolarBox`, which are
/// not ported yet, so this type has no values until those units land.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceGridElement {}

/// What a [`GridElement`] stands for (the derived `SmGridElement` classes, and
/// what their `GetElement` returns).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GridItem<O> {
    /// `SmSurfaceGridElement`: one leaf of a surface subdivision.
    Surface(SurfaceGridElement),
    /// `SmPolygonGridElement`: a leaf node of an `SmBrepCache` surface tree.
    Polygon(NodeId),
    /// `SmHierarchyGridElement`: back reference to its owning hierarchy element
    /// (`None` until the element is added to a grid).
    Hierarchy(Option<HierarchyId>),
    /// `SmUserGridElement`: anything the application stores.
    User(O),
}

/// One element registered in a grid (`SmGridElement`); one element may sit in
/// several voxels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridElement<O> {
    /// `m_lMark`: matched with the grid's current mark during a ray walk.
    mark: u64,
    /// The derived-class payload (and so `m_lType`).
    item: GridItem<O>,
    /// `m_sSphereCenter`: centre of the bounding sphere.
    sphere_center: Point3,
    /// `m_dSphereRadius`: radius of the bounding sphere.
    sphere_radius: f64,
    /// `m_sBBox`: bounding box.
    bbox: Extent3d,
}

impl<O: Copy> GridElement<O> {
    /// A fresh element as left by an `Init` (upstream pool memory: mark 0, the
    /// rest set by the caller).
    const fn init(item: GridItem<O>) -> Self {
        Self {
            mark: 0,
            item,
            sphere_center: Vec3::UNDEF,
            sphere_radius: 0.0,
            bbox: Extent3d::new(),
        }
    }

    /// `SmPolygonGridElement::Init`.
    const fn polygon(node: NodeId) -> Self {
        Self::init(GridItem::Polygon(node))
    }

    /// `SmHierarchyGridElement::Init`.
    const fn hierarchy(owner: Option<HierarchyId>) -> Self {
        Self::init(GridItem::Hierarchy(owner))
    }

    /// `SmUserGridElement::Init`.
    const fn user(object: O) -> Self {
        Self::init(GridItem::User(object))
    }

    /// The derived type code (`GetType`).
    #[must_use]
    pub const fn type_code(&self) -> i64 {
        match self.item {
            GridItem::Surface(_) => SURFACE_GRID_ELEMENT_TYPE,
            GridItem::Polygon(_) => POLYGON_GRID_ELEMENT_TYPE,
            GridItem::Hierarchy(_) => HIERARCHY_GRID_ELEMENT_TYPE,
            GridItem::User(_) => USER_GRID_ELEMENT_TYPE,
        }
    }

    /// What this element stands for (`GetElement` of the derived classes).
    #[must_use]
    pub const fn item(&self) -> GridItem<O> {
        self.item
    }

    /// Centre of the bounding sphere (`GetSphereCenter`).
    #[must_use]
    pub const fn sphere_center(&self) -> Point3 {
        self.sphere_center
    }

    /// Radius of the bounding sphere (`GetSphereRadius`).
    #[must_use]
    pub const fn sphere_radius(&self) -> f64 {
        self.sphere_radius
    }

    /// Bounding box (`GetBoundingBox`).
    #[must_use]
    pub const fn bounding_box(&self) -> &Extent3d {
        &self.bbox
    }

    /// The ray-walk mark (`m_lMark`).
    #[must_use]
    pub const fn mark(&self) -> u64 {
        self.mark
    }
}

/// An element that can be expanded into many grid elements (`SmHierarchyElement`):
/// currently only a surface whose subdivision leaves go into the voxels
/// (`SmSurfaceHierarchyElement`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HierarchyElement<O> {
    /// `m_vGridElement`: box, bounding sphere and back reference.
    grid_element: GridElement<O>,
    /// `m_dExpansionTolerance`: how much to grow boxes by.
    expansion_tolerance: f64,
    /// `SmSurfaceHierarchyElement::m_pSurface`; `None` for the abstract base class.
    surface: Option<O>,
}

impl<O: Copy> HierarchyElement<O> {
    /// A base-class hierarchy element (`SmHierarchyElement(dExpansionTolerance)`);
    /// expanding it fails, as upstream's "pure virtual" body.
    #[must_use]
    pub const fn new(expansion_tolerance: f64) -> Self {
        Self {
            grid_element: GridElement::hierarchy(None),
            expansion_tolerance,
            surface: None,
        }
    }

    /// A surface hierarchy element
    /// (`SmSurfaceHierarchyElement(pSurface, dExpansionTolerance)`).
    #[must_use]
    pub const fn new_surface(surface: O, expansion_tolerance: f64) -> Self {
        Self {
            grid_element: GridElement::hierarchy(None),
            expansion_tolerance,
            surface: Some(surface),
        }
    }

    /// The embedded grid element (`GetHierarchyGridElement`).
    #[must_use]
    pub const fn grid_element(&self) -> &GridElement<O> {
        &self.grid_element
    }

    /// The expansion tolerance (`m_dExpansionTolerance`).
    #[must_use]
    pub const fn expansion_tolerance(&self) -> f64 {
        self.expansion_tolerance
    }

    /// The surface of a surface hierarchy element (`GetSurface`).
    #[must_use]
    pub const fn surface(&self) -> Option<O> {
        self.surface
    }
}

/// One cell of a [`Grid`] (`SmVoxel`). `Default` is `SmVoxel::Init`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Voxel {
    /// `m_pHierarchyElements`: hierarchy elements whose box meets this voxel
    /// (`None` until the first one is added).
    hierarchy_elements: Option<Vec<HierarchyId>>,
    /// `m_pElements`: grid elements whose box meets this voxel (`None` until the
    /// first one is added).
    elements: Option<Vec<ElementId>>,
}

impl Voxel {
    /// Hierarchy elements in this voxel, in insertion order
    /// (`GetHierarchyElements`; `None` = null list).
    #[must_use]
    pub fn hierarchy_elements(&self) -> Option<&[HierarchyId]> {
        self.hierarchy_elements.as_deref()
    }

    /// Grid elements in this voxel, in insertion order (`GetGridElements`;
    /// `None` = null list).
    #[must_use]
    pub fn grid_elements(&self) -> Option<&[ElementId]> {
        self.elements.as_deref()
    }
}

/// The 3DDDA ray-walk state of a grid (set by [`Grid::setup_3ddda`], advanced by
/// [`Grid::step_3ddda`]).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Dda {
    /// `m_sRayPoint`: ray start.
    pub ray_point: Point3,
    /// `m_sRayVector`: unit ray direction.
    pub ray_vector: Vec3,
    /// `m_dRayT`: ray parameter where the current voxel is entered.
    pub ray_t: f64,
    /// `m_sCurrPoss`: the point at [`Dda::ray_t`].
    pub curr_pos: Point3,
    /// `m_dNextT`: ray parameters of the next x, y and z voxel planes.
    pub next_t: Vec3,
    /// `m_dDeltaT`: ray parameter between consecutive x, y and z planes.
    pub delta_t: Vec3,
    /// `m_sDeltaXVec`: ray step between consecutive x planes.
    pub delta_x_vec: Vec3,
    /// `m_sDeltaYVec`: ray step between consecutive y planes.
    pub delta_y_vec: Vec3,
    /// `m_sDeltaZVec`: ray step between consecutive z planes.
    pub delta_z_vec: Vec3,
    /// `m_sNextX`: next x-plane crossing.
    pub next_x: Point3,
    /// `m_sNextY`: next y-plane crossing.
    pub next_y: Point3,
    /// `m_sNextZ`: next z-plane crossing.
    pub next_z: Point3,
    /// `m_lStepX/Y/Z`: voxel index increments (`1`, `-1`).
    pub step: [i64; 3],
    /// `m_lOutX/Y/Z`: voxel index that means the ray left the grid (`-1` or the size).
    pub out: [i64; 3],
    /// `m_lAddress`: indices of the current voxel.
    pub address: [i64; 3],
}

/// Output of [`Grid::step_3ddda`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StepHits {
    /// `rbExitedGrid`: the ray left the grid.
    pub exited_grid: bool,
    /// `rSortedElementsInVoxel`: elements whose boxes the ray crosses, by
    /// increasing entry parameter.
    pub elements: Vec<ElementId>,
    /// `rMinDistances`: entry parameter of each element box.
    pub min_distances: Vec<f64>,
    /// `rMaxDistances`: exit parameter of each element box.
    pub max_distances: Vec<f64>,
}

/// What to register with [`Grid::add_element`] (upstream `pElement` and its
/// `SmGridElementType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementSource<O> {
    /// `SM_GE_SURFACE`: a leaf node of a surface subdivision tree.
    Surface(NodeId),
    /// `SM_GE_POLYBREP`: a leaf node of a poly B-rep cache tree.
    PolyBrep(NodeId),
    /// `SM_GE_USERDATA`: application data.
    UserData(O),
}

/// A uniform grid of voxels over a box (`SmGrid`).
#[derive(Debug, Clone, PartialEq)]
pub struct Grid<O> {
    /// `m_sBBox`: extent of the grid.
    bbox: Extent3d,
    /// `m_lSize`: voxels along x, y and z.
    size: [usize; 3],
    /// `m_sVoxelSize`: size of one voxel.
    voxel_size: Vec3,
    /// `m_dVoxelTolerance`.
    voxel_tolerance: f64,
    /// `m_lCurrentMark`.
    current_mark: u64,
    /// `m_aVoxels`, flattened (`x`-major).
    voxels: Vec<Voxel>,
    /// Every grid element, in creation order (the element pools).
    elements: Vec<GridElement<O>>,
    /// `m_pHierarchyElements`: every hierarchy element, in insertion order.
    hierarchy: Vec<HierarchyElement<O>>,
    /// The 3DDDA scratch state.
    dda: Dda,
}

/// `lNumFound`/`rdTEnter`/`rdTExit` of an `IntersectLine` call whose `rdTEnter`
/// and `rdTExit` started at `0.0`.
const fn line_hit(hit: LineHit) -> (u32, f64, f64) {
    match hit {
        LineHit::Miss => (0, 0.0, 0.0),
        LineHit::Touch { t } => (1, t, t),
        LineHit::Through { enter, exit } => (2, enter, exit),
    }
}

/// `sm_InsertionSort`: inserts `elem` with its interval before the first entry
/// with a larger entry parameter.
fn insertion_sort(elem: ElementId, t_enter: f64, t_exit: f64, hits: &mut StepHits) {
    let mut insert_at = hits.min_distances.len();
    for (i, &d) in hits.min_distances.iter().enumerate() {
        if t_enter < d {
            insert_at = i;
            break;
        }
    }
    hits.elements.insert(insert_at, elem);
    hits.min_distances.insert(insert_at, t_enter);
    hits.max_distances.insert(insert_at, t_exit);
}

fn bad_address(operation: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation,
        reason: "voxel address outside the grid",
    }
}

fn bad_id(operation: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation,
        reason: "unknown grid element id",
    }
}

impl<O: Copy> Grid<O> {
    /// A grid of `size[0] x size[1] x size[2]` empty voxels over `bbox`
    /// (`SmGrid(crGridBBox, lSize)`).
    ///
    /// Fails when a count is zero or the voxel total overflows.
    pub fn new(bbox: &Extent3d, size: [usize; 3]) -> Result<Self> {
        const OP: &str = "geom::grid::Grid::new";
        let [nx, ny, nz] = size;
        if nx == 0 || ny == 0 || nz == 0 {
            return Err(SolidError::InvalidInput {
                operation: OP,
                reason: "zero voxel count",
            });
        }
        let total =
            nx.checked_mul(ny)
                .and_then(|n| n.checked_mul(nz))
                .ok_or(SolidError::InvalidInput {
                    operation: OP,
                    reason: "voxel count overflows",
                })?;

        let mut voxel_size = bbox.size();
        voxel_size.x /= nx as f64;
        voxel_size.y /= ny as f64;
        voxel_size.z /= nz as f64;

        let voxel_tolerance = EFF_ZERO_SQRT * (1.0 + voxel_size.max_dimension()) / 100.0;

        Ok(Self {
            bbox: *bbox,
            size,
            voxel_size,
            voxel_tolerance,
            current_mark: 0,
            voxels: vec![Voxel::default(); total],
            elements: Vec::new(),
            hierarchy: Vec::new(),
            dda: Dda::default(),
        })
    }

    /// Extent of the grid (`m_sBBox`).
    #[must_use]
    pub const fn bbox(&self) -> &Extent3d {
        &self.bbox
    }

    /// Voxel counts along x, y and z (`m_lSize`).
    #[must_use]
    pub const fn size(&self) -> [usize; 3] {
        self.size
    }

    /// Size of one voxel (`m_sVoxelSize`).
    #[must_use]
    pub const fn voxel_size(&self) -> Vec3 {
        self.voxel_size
    }

    /// Tolerance of voxel computations (`m_dVoxelTolerance`).
    #[must_use]
    pub const fn voxel_tolerance(&self) -> f64 {
        self.voxel_tolerance
    }

    /// The current ray-walk mark (`m_lCurrentMark`).
    #[must_use]
    pub const fn current_mark(&self) -> u64 {
        self.current_mark
    }

    /// The 3DDDA ray-walk state.
    #[must_use]
    pub const fn dda(&self) -> &Dda {
        &self.dda
    }

    /// Every grid element, in creation order.
    #[must_use]
    pub fn elements(&self) -> &[GridElement<O>] {
        &self.elements
    }

    /// Every hierarchy element, in insertion order (`m_pHierarchyElements`).
    #[must_use]
    pub fn hierarchy_elements(&self) -> &[HierarchyElement<O>] {
        &self.hierarchy
    }

    /// The grid element `id`.
    pub fn element(&self, id: ElementId) -> Result<&GridElement<O>> {
        self.elements
            .get(id.0)
            .ok_or_else(|| bad_id("geom::grid::Grid::element"))
    }

    /// The hierarchy element `id`.
    pub fn hierarchy_element(&self, id: HierarchyId) -> Result<&HierarchyElement<O>> {
        self.hierarchy
            .get(id.0)
            .ok_or_else(|| bad_id("geom::grid::Grid::hierarchy_element"))
    }

    /// Flat index of a signed voxel address, if it is inside the grid.
    fn voxel_index(&self, a: [i64; 3]) -> Option<usize> {
        let ix = usize::try_from(a[0]).ok()?;
        let iy = usize::try_from(a[1]).ok()?;
        let iz = usize::try_from(a[2]).ok()?;
        let [nx, ny, nz] = self.size;
        if ix >= nx || iy >= ny || iz >= nz {
            return None;
        }
        Some((ix * ny + iy) * nz + iz)
    }

    /// The voxel at `address` (`GetVoxel`).
    pub fn voxel(&self, address: VoxelAddress) -> Result<&Voxel> {
        const OP: &str = "geom::grid::Grid::voxel";
        let a = signed(address).ok_or_else(|| bad_address(OP))?;
        self.voxel_index(a)
            .and_then(|i| self.voxels.get(i))
            .ok_or_else(|| bad_address(OP))
    }

    /// The box of the voxel at `address` (`GetVoxelBBox`); any address is
    /// accepted.
    #[must_use]
    pub fn voxel_bbox(&self, address: [i64; 3]) -> Extent3d {
        let lo = self.bbox.min();
        let min = Point3::new(
            lo.x + address[0] as f64 * self.voxel_size.x,
            lo.y + address[1] as f64 * self.voxel_size.y,
            lo.z + address[2] as f64 * self.voxel_size.z,
        );
        let max = min + self.voxel_size;
        Extent3d::from_points(min, max)
    }

    /// The voxel containing `point`, or the nearest one when `point` is outside
    /// the grid, and whether it was outside (`GetVoxelAddress`).
    #[must_use]
    pub fn voxel_address(&self, point: Point3) -> (VoxelAddress, bool) {
        let mut n = point - self.bbox.min();
        let s = self.bbox.size();
        n.x /= s.x;
        n.y /= s.y;
        n.z /= s.z;

        let outside = [n.x, n.y, n.z].iter().any(|&c| c < 0.0 || c > 1.0);

        n.x = n.x.clamp(0.0, 1.0);
        n.y = n.y.clamp(0.0, 1.0);
        n.z = n.z.clamp(0.0, 1.0);

        if n.x > 1.0 - EFF_ZERO {
            n.x -= EFF_ZERO;
        }
        if n.y > 1.0 - EFF_ZERO {
            n.y -= EFF_ZERO;
        }
        if n.z > 1.0 - EFF_ZERO {
            n.z -= EFF_ZERO;
        }
        let [nx, ny, nz] = self.size;
        let mut a = [
            (n.x * nx as f64) as usize,
            (n.y * ny as f64) as usize,
            (n.z * nz as f64) as usize,
        ];
        for (ai, ni) in a.iter_mut().zip(self.size) {
            if *ai == ni {
                *ai -= 1;
            }
        }
        (a, outside)
    }

    /// Every voxel between the voxels of `bbox`'s corners, `x` outermost
    /// (`FindVoxelsInBox`).
    #[must_use]
    pub fn find_voxels_in_box(&self, bbox: &Extent3d) -> Vec<VoxelAddress> {
        let (lo, _) = self.voxel_address(bbox.min());
        let (hi, _) = self.voxel_address(bbox.max());
        let mut out = Vec::new();
        for ix in lo[0]..=hi[0] {
            for iy in lo[1]..=hi[1] {
                for iz in lo[2]..=hi[2] {
                    out.push([ix, iy, iz]);
                }
            }
        }
        out
    }

    /// Mutable voxel at a valid address.
    fn voxel_mut(&mut self, address: VoxelAddress, op: &'static str) -> Result<&mut Voxel> {
        let i = signed(address)
            .and_then(|a| self.voxel_index(a))
            .ok_or_else(|| bad_address(op))?;
        self.voxels.get_mut(i).ok_or_else(|| bad_address(op))
    }

    /// Takes ownership of `element`, sets its grid element's box (`bbox` grown by
    /// the expansion tolerance) and bounding sphere, and lists it in every voxel
    /// that `bbox` meets (`AddHierarchyElement`).
    pub fn add_hierarchy_element(
        &mut self,
        mut element: HierarchyElement<O>,
        bbox: &Extent3d,
    ) -> Result<HierarchyId> {
        const OP: &str = "geom::grid::Grid::add_hierarchy_element";
        let id = HierarchyId(self.hierarchy.len());
        let ge = &mut element.grid_element;
        ge.bbox = *bbox;
        ge.bbox.expand_absolute(element.expansion_tolerance);
        let sb = ge.bbox.sphere_bound();
        ge.sphere_center = sb.center;
        ge.sphere_radius = sb.radius;
        ge.item = GridItem::Hierarchy(Some(id));
        self.hierarchy.push(element);

        for a in self.find_voxels_in_box(bbox) {
            self.voxel_mut(a, OP)?
                .hierarchy_elements
                .get_or_insert_with(Vec::new)
                .push(id);
        }
        Ok(id)
    }

    /// Makes a grid element for `source` with box `bbox` and lists it in every
    /// voxel that `bbox` meets (`AddElement`).
    pub fn add_element(&mut self, source: ElementSource<O>, bbox: &Extent3d) -> Result<ElementId> {
        const OP: &str = "geom::grid::Grid::add_element";
        let mut ge = match source {
            ElementSource::Surface(_) => {
                // SmBezierPatch::GetPseudoBox and SmBezierAux2d::GetPolarBox are ported
                // (surface::surface_cache::p1), but the patch data lives in the surface
                // cache, which a tree node reaches once the tree construction hands it
                // over (SmSurfaceGridElement::Init also needs SmPseudoBox/SmPolarBox
                // copies, SmSurface::Evaluate and SmSurface::EvaluateNormal)
                // DISPATCH GAP: SmBezierPatch::GetPseudoBox (surface.surface_cache_p1,
                // ported; no UNPORTED marker: the callee is ported, the tree cannot reach the cache's data yet)
                return Err(SolidError::Unported {
                    upstream: "SmBezierPatch::GetPseudoBox",
                    unit: "surface.surface_cache_p1",
                });
            }
            ElementSource::PolyBrep(node) => GridElement::polygon(node),
            ElementSource::UserData(o) => GridElement::user(o),
        };
        let sb = bbox.sphere_bound();
        ge.sphere_center = sb.center;
        ge.sphere_radius = sb.radius;
        ge.bbox = *bbox;
        let id = ElementId(self.elements.len());
        self.elements.push(ge);

        for a in self.find_voxels_in_box(bbox) {
            self.voxel_mut(a, OP)?
                .elements
                .get_or_insert_with(Vec::new)
                .push(id);
        }
        Ok(id)
    }

    /// Embeds the subdivision leaves of hierarchy element `id` into the voxels
    /// (`SmSurfaceHierarchyElement::ExpandHierarchy`; on a base-class element,
    /// `SmHierarchyElement::ExpandHierarchy`, which fails with `SM_ERR`).
    pub fn expand_hierarchy(&mut self, id: HierarchyId) -> Result<()> {
        const OP: &str = "geom::grid::Grid::expand_hierarchy";
        let element = self.hierarchy.get(id.0).ok_or_else(|| bad_id(OP))?;
        if element.surface.is_none() {
            return Err(SolidError::sm_err(OP));
        }
        // smsurf_GetSurfaceCache (surface::surface::p2::surface_cache) needs the
        // surface caches:
        // SmCacheMgr::GetOrCreateObjectCache makes surface caches through
        // SmSurface::CacheMakeOrValidate:
        // UNPORTED(surface.surface_p1): SmSurface::CacheMakeOrValidate
        // (then SmSurfaceCache::GetTree, SmTree::GetAllTreeNodes, and for every
        // lowest SM_AD_BEZIER_SURFACE node not classified outside:
        // AddElement(node, SM_GE_SURFACE, node box grown by the expansion tolerance))
        Err(SolidError::Unported {
            upstream: "SmSurface::CacheMakeOrValidate",
            unit: "surface.surface_p1",
        })
    }

    /// Starts a 3DDDA walk of the ray `start + t * vector` (`Setup3DDDA`): finds
    /// the first voxel and the plane steps, and increments the current mark.
    ///
    /// Returns `true` (upstream `rbHitsNothing`) when the ray misses the grid; the
    /// mark is then unchanged. Fails with `SM_ERR` when `vector` is shorter than
    /// `sqrt(EFF_ZERO)`.
    pub fn setup_3ddda(&mut self, start: Point3, vector: Vec3) -> Result<bool> {
        const OP: &str = "geom::grid::Grid::setup_3ddda";
        if vector.length_squared() < EFF_ZERO {
            return Err(SolidError::sm_err(OP));
        }

        self.dda.ray_point = start;
        self.dda.ray_vector = vector;
        self.dda.ray_vector = self.dda.ray_vector.unitize()?;
        let rp = self.dda.ray_point;
        let rv = self.dda.ray_vector;

        if self.bbox.contains_point(rp, 0.0) {
            let (a, outside) = self.voxel_address(rp);
            if outside {
                return Err(SolidError::sm_err(OP));
            }
            let voxel_box = self.voxel_bbox(signed(a).ok_or_else(|| bad_address(OP))?);
            let (_, t_enter, _) = line_hit(voxel_box.intersect_line(rp, rv)?);
            self.dda.ray_t = t_enter;
        } else {
            let (found, t_enter, _) = line_hit(self.bbox.intersect_line(rp, rv)?);
            if found == 0 {
                return Ok(true);
            }
            self.dda.ray_t = t_enter;
        }

        let test_pnt = rp + (self.dda.ray_t + self.voxel_tolerance) * rv;
        self.dda.curr_pos = rp + self.dda.ray_t * rv;

        let (a, outside) = self.voxel_address(test_pnt);
        let address = signed(a).ok_or_else(|| bad_address(OP))?;
        self.dda.address = address;
        if outside {
            return Ok(true);
        }

        let voxel_box = self.voxel_bbox(address);
        let (vmin, vmax) = (voxel_box.min(), voxel_box.max());
        let ray_t = self.dda.ray_t;
        let cur = self.dda.curr_pos;
        let vs = self.voxel_size;
        let [nx, ny, nz] = signed(self.size).ok_or_else(|| bad_address(OP))?;
        let axes = [
            AxisStep::new(ray_t, rv.x, vmin.x, vmax.x, cur.x, vs.x, nx),
            AxisStep::new(ray_t, rv.y, vmin.y, vmax.y, cur.y, vs.y, ny),
            AxisStep::new(ray_t, rv.z, vmin.z, vmax.z, cur.z, vs.z, nz),
        ];
        let d = &mut self.dda;
        let [ax, ay, az] = axes;
        d.next_t = Vec3::new(ax.next_t, ay.next_t, az.next_t);
        d.delta_t = Vec3::new(ax.delta_t, ay.delta_t, az.delta_t);
        for ((axis, step), out) in axes.iter().zip(&mut d.step).zip(&mut d.out) {
            // an axis the ray does not move along keeps its step and exit index
            if let Some((s, o)) = axis.step_out {
                *step = s;
                *out = o;
            }
        }

        d.delta_x_vec = rv * d.delta_t.x;
        d.delta_y_vec = rv * d.delta_t.y;
        d.delta_z_vec = rv * d.delta_t.z;

        if d.next_t.x < BIG_DOUBLE {
            d.next_x = rp + d.next_t.x * rv;
        }
        if d.next_t.y < BIG_DOUBLE {
            d.next_y = rp + d.next_t.y * rv;
        }
        if d.next_t.z < BIG_DOUBLE {
            d.next_z = rp + d.next_t.z * rv;
        }

        self.current_mark = self.current_mark.wrapping_add(1);
        Ok(false)
    }

    /// Walks the ray from voxel to voxel until some voxel holds elements whose
    /// boxes the ray crosses no later than `min_found_t`, or the ray leaves the
    /// grid (`Step3DDDA`). Each element is reported once per walk (it is marked
    /// with the current mark).
    pub fn step_3ddda(&mut self, min_found_t: f64) -> Result<StepHits> {
        const OP: &str = "geom::grid::Grid::step_3ddda";
        let mut hits = StepHits::default();
        let rp = self.dda.ray_point;
        let rv = self.dda.ray_vector;
        let mark = self.current_mark;

        loop {
            let vi = self
                .voxel_index(self.dda.address)
                .ok_or_else(|| bad_address(OP))?;
            let voxel = self.voxels.get(vi).ok_or_else(|| bad_address(OP))?;

            if let Some(list) = &voxel.elements {
                for &eid in list {
                    let e = self.elements.get_mut(eid.0).ok_or_else(|| bad_id(OP))?;

                    if e.mark == mark {
                        continue;
                    }

                    let vec_to_center = e.sphere_center - rp;
                    let center_t = vec_to_center.dot(rv);
                    let point_on_ray = rp + center_t * rv;
                    let vec_to = point_on_ray - e.sphere_center;
                    let dist_sq = vec_to.length_squared();

                    if dist_sq > e.sphere_radius * e.sphere_radius {
                        continue;
                    }

                    let (found, t_enter, t_exit) = line_hit(e.bbox.intersect_line(rp, rv)?);

                    if found == 0 {
                        continue;
                    }
                    if t_exit < -EFF_ZERO {
                        continue;
                    }
                    if t_enter > min_found_t {
                        continue;
                    }

                    e.mark = mark;
                    insertion_sort(eid, t_enter, t_exit, &mut hits);
                }
            }

            let d = &mut self.dda;
            if d.next_t.x < d.next_t.y && d.next_t.x < d.next_t.z {
                d.address[0] += d.step[0];
                if d.address[0] == d.out[0] {
                    hits.exited_grid = true;
                    return Ok(hits);
                }
                d.ray_t = d.next_t.x;
                d.next_t.x += d.delta_t.x;
                d.curr_pos = d.next_x;
                d.next_x += d.delta_x_vec;
            } else if d.next_t.y < d.next_t.z {
                d.address[1] += d.step[1];
                if d.address[1] == d.out[1] {
                    hits.exited_grid = true;
                    return Ok(hits);
                }
                d.ray_t = d.next_t.y;
                d.next_t.y += d.delta_t.y;
                d.curr_pos = d.next_y;
                d.next_y += d.delta_y_vec;
            } else {
                d.address[2] += d.step[2];
                if d.address[2] == d.out[2] {
                    hits.exited_grid = true;
                    return Ok(hits);
                }
                d.ray_t = d.next_t.z;
                d.next_t.z += d.delta_t.z;
                d.curr_pos = d.next_z;
                d.next_z += d.delta_z_vec;
            }

            if !hits.elements.is_empty() {
                break;
            }
        }
        Ok(hits)
    }
}

/// One axis of the 3DDDA set-up in [`Grid::setup_3ddda`].
#[derive(Clone, Copy)]
struct AxisStep {
    /// `m_dNextT` component: ray parameter of the next voxel plane on this axis.
    next_t: f64,
    /// `m_dDeltaT` component: ray parameter between consecutive planes.
    delta_t: f64,
    /// `(m_lStep, m_lOut)` for this axis; `None` when the ray does not move along it.
    step_out: Option<(i64, i64)>,
}

impl AxisStep {
    /// The set-up for ray component `rv` entering the voxel `[vmin, vmax]` at
    /// `cur` (ray parameter `ray_t`), with voxel size `vs` and `n` voxels.
    fn new(ray_t: f64, rv: f64, vmin: f64, vmax: f64, cur: f64, vs: f64, n: i64) -> Self {
        if rv.abs() < EFF_ZERO {
            Self {
                next_t: BIG_DOUBLE,
                delta_t: 0.0,
                step_out: None,
            }
        } else if rv < 0.0 {
            Self {
                next_t: ray_t + (vmin - cur) / rv,
                delta_t: vs / -rv,
                step_out: Some((-1, -1)),
            }
        } else {
            Self {
                next_t: ray_t + (vmax - cur) / rv,
                delta_t: vs / rv,
                step_out: Some((1, n)),
            }
        }
    }
}

/// A voxel address (or count triple) as upstream `long`s.
fn signed(a: [usize; 3]) -> Option<[i64; 3]> {
    Some([
        i64::try_from(a[0]).ok()?,
        i64::try_from(a[1]).ok()?,
        i64::try_from(a[2]).ok()?,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_grid(n: usize) -> Grid<u32> {
        let bbox = Extent3d::from_bounds(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        Grid::new(&bbox, [n, n, n]).unwrap()
    }

    #[test]
    fn construction() {
        let bbox = Extent3d::from_bounds(0.0, 0.0, 0.0, 2.0, 4.0, 6.0);
        let g: Grid<u32> = Grid::new(&bbox, [2, 4, 3]).unwrap();
        assert_eq!(g.voxel_size(), Vec3::new(1.0, 1.0, 2.0));
        assert_eq!(g.voxel_tolerance(), EFF_ZERO_SQRT * 3.0 / 100.0);
        assert!(g.voxel([1, 3, 2]).is_ok());
        assert!(g.voxel([2, 0, 0]).is_err());
        assert!(Grid::<u32>::new(&bbox, [0, 1, 1]).is_err());
        assert!(Grid::<u32>::new(&bbox, [usize::MAX, 2, 1]).is_err());
    }

    #[test]
    fn voxel_address_and_box() {
        let g = unit_grid(4);
        assert_eq!(
            g.voxel_address(Point3::new(0.3, 0.6, 0.99)),
            ([1, 2, 3], false)
        );
        assert_eq!(
            g.voxel_address(Point3::new(1.0, 1.0, 1.0)),
            ([3, 3, 3], false)
        );
        assert_eq!(
            g.voxel_address(Point3::new(-5.0, 2.0, 0.5)),
            ([0, 3, 2], true)
        );
        let b = g.voxel_bbox([1, 2, 3]);
        assert_eq!(b.min(), Point3::new(0.25, 0.5, 0.75));
        assert_eq!(b.max(), Point3::new(0.5, 0.75, 1.0));
    }

    #[test]
    fn find_and_add() {
        let mut g = unit_grid(4);
        let b = Extent3d::from_bounds(0.1, 0.1, 0.1, 0.4, 0.2, 0.1);
        assert_eq!(g.find_voxels_in_box(&b), vec![[0, 0, 0], [1, 0, 0]]);
        let id = g.add_element(ElementSource::UserData(7), &b).unwrap();
        assert_eq!(g.voxel([1, 0, 0]).unwrap().grid_elements(), Some(&[id][..]));
        assert_eq!(g.voxel([2, 0, 0]).unwrap().grid_elements(), None);
        assert_eq!(g.element(id).unwrap().type_code(), USER_GRID_ELEMENT_TYPE);
        assert_eq!(g.element(id).unwrap().item(), GridItem::User(7));

        let h = g
            .add_hierarchy_element(HierarchyElement::new(0.01), &b)
            .unwrap();
        let he = g.hierarchy_element(h).unwrap();
        assert_eq!(he.grid_element().item(), GridItem::Hierarchy(Some(h)));
        assert_eq!(he.grid_element().bounding_box().min().x, 0.1 - 0.01);
        assert!(matches!(
            g.expand_hierarchy(h),
            Err(SolidError::Upstream {
                code: crate::error::SM_ERR,
                ..
            })
        ));
        let hs = g
            .add_hierarchy_element(HierarchyElement::new_surface(3, 0.0), &b)
            .unwrap();
        assert!(matches!(
            g.expand_hierarchy(hs),
            Err(SolidError::Unported { .. })
        ));
        assert!(matches!(
            g.add_element(ElementSource::Surface(NodeId(0)), &b),
            Err(SolidError::Unported { .. })
        ));
    }

    #[test]
    fn ray_walk() {
        let mut g = unit_grid(4);
        let near = Extent3d::from_bounds(0.6, 0.45, 0.45, 0.7, 0.55, 0.55);
        let far = Extent3d::from_bounds(0.1, 0.4, 0.4, 0.2, 0.6, 0.6);
        let a = g.add_element(ElementSource::UserData(1), &far).unwrap();
        let b = g.add_element(ElementSource::UserData(2), &near).unwrap();
        // Ray along -x through the middle.
        assert!(!g
            .setup_3ddda(Point3::new(2.0, 0.5, 0.5), Vec3::new(-1.0, 0.0, 0.0))
            .unwrap());
        assert_eq!(g.dda().ray_t, 1.0);
        assert_eq!(g.dda().address, [3, 2, 2]);
        let h = g.step_3ddda(BIG_DOUBLE).unwrap();
        assert!(!h.exited_grid);
        assert_eq!(h.elements, vec![b]);
        assert!((h.min_distances[0] - 1.3).abs() < 1e-12);
        // The last voxel's hits come back together with the exit.
        let h = g.step_3ddda(BIG_DOUBLE).unwrap();
        assert!(h.exited_grid);
        assert_eq!(h.elements, vec![a]);
        assert!(g.step_3ddda(BIG_DOUBLE).is_err());

        // Misses and bad directions.
        assert!(g
            .setup_3ddda(Point3::new(2.0, 2.0, 2.0), Vec3::new(1.0, 0.0, 0.0))
            .unwrap());
        assert!(g
            .setup_3ddda(Point3::ZERO, Vec3::new(1e-7, 0.0, 0.0))
            .is_err());
    }

    #[test]
    fn insertion_order() {
        let mut h = StepHits::default();
        insertion_sort(ElementId(0), 2.0, 3.0, &mut h);
        insertion_sort(ElementId(1), 1.0, 3.0, &mut h);
        insertion_sort(ElementId(2), 2.0, 4.0, &mut h);
        assert_eq!(h.elements, vec![ElementId(1), ElementId(0), ElementId(2)]);
        assert_eq!(h.max_distances, vec![3.0, 3.0, 4.0]);
    }
}
