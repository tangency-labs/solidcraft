//! Shell and region topology, with typed arena links and native traversal order.
//!
//! Upstream: `source/SMLib/inc/SmShell.h`, `source/SMLib/src/SmShell.cpp`,
//! `source/SMLib/inc/SmRegion.h`, `source/SMLib/src/SmRegion.cpp`.
//! The `SmEdge`/`SmVertex` callees of wire-edge traversal are called through their
//! owners' ports (`topology::edge`, `topology::vertex`), not re-ported here;
//! `source/SMLib/inc/SmTopology.h` supplies literal sibling links and
//! `source/SMLib/inc/SmObject.h` defines the checked `SM_CAST_PTR` semantics.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`ShellType`], [`ShellMembers`] | `m_tShellType`, typed `m_pList` contents |
//! | [`Shell::new`], [`Region::new`], implicit `Drop` | constructors, destructors |
//! | [`Shell::type_string`], [`Shell::class_string`], [`Shell::is_kind_of`], [`Region::type_string`], [`Region::class_string`], [`Region::is_kind_of`] | `SM_COMMON` names and full base-class chain |
//! | [`Shell::get_faceuses`], [`Shell::first_faceuse`] | `GetFaceuses`, `GetFirstFaceuse` |
//! | [`Shell::wire_edge`], [`Shell::vertex`] | `GetWireEdge`, `GetVertex` |
//! | [`Shell::is_closed`], [`Shell::mated_shells`] | `IsClosed`, `GetMatedShells` |
//! | [`shell_region`], [`shell_brep`] | `GetRegion`, `GetBrep` |
//! | [`RegionLookup`], [`RegionRef`] | caller-supplied resolution of implicit native region pointers |
//! | [`wire_edges`] | `SmShell::GetWireEdges` |
//! | [`ray_fire`] | `SmShell::RayFire` (partial output state retained) |
//! | [`transfer_children`] | `SmShell::TransferChildrenTo` |
//! | [`move_faceuse`] | `SmOwningTopology::Remove` + `PostInsert` of a face use (typed list) |
//! | [`post_insert_faceuse`] | `SmOwningTopology::PostInsert` of a new face use (typed list) |
//! | [`shell_is_degenerate`], [`region_is_degenerate`] | `IsDegenerate` (native default tolerance) |
//! | `shell_bounds_for_degeneracy`, `region_bounds_for_degeneracy` | default-tight `CalculateBoundingBox` (`query::face::p1`), status kept beside the partial box |
//! | [`faceuse_sibling_links`] | typed `SmTopology::GetNext` / `GetLast` for face uses |
//! | `topology::edge::{vertices, edgeuses, is_wire}`, `topology::vertex::edges` | `SmEdge::GetVertices`, `GetEdgeuses`, `IsWire`, `SmVertex::GetEdges` (callees, no filters) |
//! | [`Region::get_shells`], [`Region::is_void`], [`Region::set_is_void`] | `GetShells`, `IsVoid`, `SetIsVoid` |
//! | [`Region::is_infinite_region`] | `IsInfiniteRegion` |
//! | [`Shell::is_inner_shell`], [`order_region_shells`] | `IsInnerShell`, `OrderShells` |
//! | [`region_remove_shell`] | `SmOwningTopology::Remove` of a shell from its region |
//! | [`region_post_insert_shell`] | `SmOwningTopology::PostInsert` of a shell into a region (CO-PORTED, `topology.brep_p3`) |
//! | [`shell_remove_faceuse`], [`shell_post_insert_faceuse`] | `SmOwningTopology::Remove` / `PostInsert` of one face use (CO-PORTED, `topology.brep_p3`) |
//! | [`shell_list_size`] | `SmShell::GetSize` (`m_lListSize`; CO-PORTED, `topology.brep_p3`) |
//!
//! Deviations: circular lists are typed vectors in head-first order, as in the
//! shared Brep arena model. `GetWireEdges`, `GetMatedShells` and `RayFire` claim and
//! lock a context mark (`SmNewMarkAndLock`) and mark through
//! [`crate::topology::topology_traverser::Marks`] (per-call, as the mark values on the
//! objects are unique per claim), advancing the kernel context's mark counters as upstream. Incompatible array reinterpret casts return `InvalidInput`;
//! checked member casts retain native NULL. Native
//! notification/logging side effects are not represented by these arena records.
//! Face-use sibling links survive a cleared owner head, as in native self-transfer.
//! `SmOwningTopology::Remove` / `PostInsert` of a face use are CO-PORTED on the
//! typed face-use list (`move_faceuse`; unit `topology.proto_topology_p1` works on
//! object-key records). `SmBrep::GetEdges` (unit `topology.brep_p1`) is its owner's
//! port `topology::brep::p1::edges` (the `m_pEdgeListHead` list order), which replaced
//! the earlier co-port.
//! `TransferChildrenTo` ignores the status of `Remove`/`PostInsert` (`SE(...)`) and goes on;
//! here a face use whose owner is not the source (or a `Remove` that cannot find it)
//! returns `InvalidInput` instead. That state needs an inconsistent graph (a face use
//! listed in a shell that its back link does not name); native code would corrupt
//! the lists. Face-use sibling links, once `Some` (`Faceuse::sibling_links`), win
//! over `Shell::members`: code that edits `members` directly (e.g. a later unit that
//! rebuilds face-use lists) must keep them in step or reset them to `None`.
//! There is no recursion. Missing external callees return explicit `Unported` errors.
//! The single-member `Remove` / `PostInsert` co-ports (`topology.brep_p3`, PR #204)
//! return upstream's `SM_ERR` of `Remove` (a member not in this list) as
//! [`SolidError::sm_err`], which callers that ignore it can tell apart. `PostInsert`
//! does no checks upstream; here it returns `InvalidInput` for a member that some list
//! still holds (upstream would splice it into a second list), or for a face use
//! going into a shell whose head is a wire or vertex use.

use crate::error::{Result, SolidError};
use crate::geom::context::{with_mark_lock, ALL_MARKS};
use crate::geom::object::Shape;
use crate::kernel::Kernel;
use crate::query::brep::{self as qbrep, BoundingBoxOptions};
use crate::topology::proto_topology::p1::owning_topology;
use crate::topology::topology_traverser::{Entity, Marks};
use crate::topology::{
    Brep, EdgeId, EdgeuseId, FaceuseId, Region, RegionId, Shell, ShellId, VertexId, VertexuseId,
};

/// The native shell member class, independently stored from its list head.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShellType {
    /// `SmUnknown_TYPE` (16999), the constructor value.
    #[default]
    Unknown,
    /// `SmVertexuse_TYPE` (16006).
    Vertex,
    /// `SmEdgeuse_TYPE` (16005).
    Wire,
    /// `SmFaceuse_TYPE` (16022).
    Faceuse,
    /// Other values preserved by `SetShellType` without reinterpretation.
    Other(i64),
}

impl ShellType {
    /// The exact native type code.
    #[must_use]
    pub const fn code(self) -> i64 {
        match self {
            Self::Unknown => 16999,
            Self::Vertex => 16006,
            Self::Wire => 16005,
            Self::Faceuse => 16022,
            Self::Other(code) => code,
        }
    }
    /// Retain an arbitrary native caller's shell type.
    #[must_use]
    pub const fn from_code(code: i64) -> Self {
        match code {
            16999 => Self::Unknown,
            16006 => Self::Vertex,
            16005 => Self::Wire,
            16022 => Self::Faceuse,
            other => Self::Other(other),
        }
    }
}

/// A checked representation of the shell's native list head and members.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ShellMembers {
    /// NULL list head.
    #[default]
    Empty,
    /// Face-use circular list, from its current head.
    Faceuses(Vec<FaceuseId>),
    /// First wire edge use (the edge owns its radial list).
    Edgeuse(EdgeuseId),
    /// Vertex use of a vertex shell.
    Vertexuse(VertexuseId),
}

const fn invalid_members() -> SolidError {
    SolidError::InvalidInput {
        operation: "topology::shell",
        reason: "shell type and native member class disagree",
    }
}

impl Shell {
    /// Empty, unknown shell (`SmShell()`).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    /// Native class type (`SmShell_TYPE`).
    #[must_use]
    pub const fn type_code() -> i64 {
        16013
    }
    /// Native class name.
    #[must_use]
    pub const fn class_string() -> &'static str {
        "SmShell"
    }
    /// Native type name.
    #[must_use]
    pub const fn type_string() -> &'static str {
        "SmShell_TYPE"
    }
    /// Native type/base-class membership, using the shared owning-topology port.
    #[must_use]
    pub const fn is_kind_of(code: i64) -> bool {
        code == Self::type_code() || owning_topology::is_kind_of(code)
    }
    /// Whether this is a vertex-use shell.
    #[must_use]
    pub const fn is_vertex_shell(&self) -> bool {
        self.shell_type.code() == 16006
    }
    /// Whether this is a wire-edge shell.
    #[must_use]
    pub const fn is_wire_shell(&self) -> bool {
        self.shell_type.code() == 16005
    }
    /// Whether this contains face uses.
    #[must_use]
    pub const fn is_faceuse_shell(&self) -> bool {
        self.shell_type.code() == 16022
    }
    /// Whether its type is the native unknown value.
    #[must_use]
    pub const fn is_unknown_shell(&self) -> bool {
        self.shell_type.code() == 16999
    }
    /// Change only the native type field, retaining its existing list head.
    pub fn set_shell_type(&mut self, code: i64) {
        self.shell_type = ShellType::from_code(code);
    }
    /// Face uses in native list order; empty for every other shell type.
    ///
    /// # Errors
    /// A native list head of an incompatible class.
    pub fn get_faceuses(&self) -> Result<Vec<FaceuseId>> {
        if !self.is_faceuse_shell() {
            return Ok(Vec::new());
        }
        match &self.members {
            ShellMembers::Empty => Ok(Vec::new()),
            ShellMembers::Faceuses(ids) => Ok(ids.clone()),
            _ => Err(invalid_members()),
        }
    }
    /// First face use; native logging-only errors retain the NULL result.
    ///
    /// A head-class mismatch retains native NULL rather than becoming an error.
    ///
    /// # Errors
    /// This compatibility result succeeds; native logging-only errors yield `None`.
    pub fn first_faceuse(&self) -> Result<Option<FaceuseId>> {
        if !self.is_faceuse_shell() {
            return Ok(None);
        }
        // SM_CAST_PTR checks the actual head class and returns NULL on mismatch.
        // Unlike the reinterpret-cast GetFaceuses array, this is a defined path.
        Ok(match &self.members {
            ShellMembers::Faceuses(ids) => ids.first().copied(),
            _ => None,
        })
    }
    /// First wire edge, preserving NULL for empty or other-kind shells.
    ///
    /// # Errors
    /// Dangling arena references; a head-class mismatch retains native NULL.
    pub fn wire_edge(&self, brep: &Brep) -> Result<Option<EdgeId>> {
        if !self.is_wire_shell() {
            return Ok(None);
        }
        match self.members {
            ShellMembers::Edgeuse(id) => Ok(brep.edgeuse(id)?.edge),
            _ => Ok(None),
        }
    }
    /// Vertex of a vertex shell, preserving NULL for empty or other-kind shells.
    ///
    /// # Errors
    /// Dangling arena references; a head-class mismatch retains native NULL.
    pub fn vertex(&self, brep: &Brep) -> Result<Option<VertexId>> {
        if !self.is_vertex_shell() {
            return Ok(None);
        }
        match self.members {
            ShellMembers::Vertexuse(id) => Ok(brep.vertexuse(id)?.vertex),
            _ => Ok(None),
        }
    }
    /// Whether a face in this shell has a mate belonging to a different shell.
    ///
    /// # Errors
    /// Invalid list classes or dangling face-use/mate references.
    pub fn is_closed(&self, brep: &Brep, id: ShellId) -> Result<bool> {
        if !self.is_faceuse_shell() {
            return Ok(false);
        }
        for faceuse in self.get_faceuses()? {
            if let Some(mate) = brep.faceuse(faceuse)?.mate {
                if brep.faceuse(mate)?.shell != Some(id) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
    /// Distinct mate shells in first-seen face-use order. Like `SmShell::GetMatedShells`
    /// this claims and locks a context mark (`SmNewMarkAndLock`, all marks) for the
    /// duration of the call, so the context's mark counters advance.
    ///
    /// # Errors
    /// Missing native mates/owners or dangling arena references; every mark is still
    /// released.
    pub fn mated_shells(&self, kernel: &mut Kernel, brep: &Brep) -> Result<Vec<ShellId>> {
        let mut output = Vec::new();
        if !self.is_faceuse_shell() {
            return Ok(output);
        }
        with_mark_lock(kernel, ALL_MARKS, |kernel, mark| {
            let mut marks = Marks::default();
            for faceuse in self.get_faceuses()? {
                let mate = brep.faceuse(faceuse)?.mate.ok_or(invalid_members())?;
                let shell = brep.faceuse(mate)?.shell.ok_or(invalid_members())?;
                brep.shell(shell)?;
                if marks.is_marked(kernel, Entity::Shell(shell), mark) {
                    continue;
                }
                marks.mark(kernel, Entity::Shell(shell), mark);
                output.push(shell);
            }
            Ok(())
        })??;
        Ok(output)
    }
    /// Classify whether this is an inner shell: `SmTopologyTraverser::IsInnerShell`
    /// on this shell's face uses (`GetFaceuses`).
    ///
    /// # Errors
    /// Invalid member classes and the errors of
    /// [`crate::topology::topology_traverser::is_inner_shell`] (its `SM_ERR` for an
    /// empty or inconclusive set, `Unported` from its ray intersections).
    pub fn is_inner_shell(&self, kernel: &mut Kernel, brep: &Brep) -> Result<bool> {
        let faceuses = self.get_faceuses()?;
        crate::topology::topology_traverser::is_inner_shell(kernel, brep, &faceuses)
    }
}

/// Owning region (`SmShell::GetRegion`).
///
/// # Errors
/// Dangling shell id.
pub fn shell_region(brep: &Brep, id: ShellId) -> Result<Option<RegionId>> {
    Ok(brep.shell(id)?.region)
}

/// B-rep of a shell's owning region (`SmShell::GetBrep`).
///
/// # Errors
/// Dangling shell or region id.
pub fn shell_brep(brep: &Brep, id: ShellId) -> Result<Option<&Brep>> {
    match shell_region(brep, id)? {
        None => Ok(None),
        Some(region) => Ok(brep.region(region)?.in_brep.then_some(brep)),
    }
}

impl Region {
    /// Empty non-void region (`SmRegion()`).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    /// Native region type.
    #[must_use]
    pub const fn type_code() -> i64 {
        16012
    }
    /// Native class name.
    #[must_use]
    pub const fn class_string() -> &'static str {
        "SmRegion"
    }
    /// Native type name.
    #[must_use]
    pub const fn type_string() -> &'static str {
        "SmRegion_TYPE"
    }
    /// Native type/base-class membership.
    #[must_use]
    pub const fn is_kind_of(code: i64) -> bool {
        code == Self::type_code() || owning_topology::is_kind_of(code)
    }
    /// Native void state, preserving `FALSE` (0), `TRUE` (1), and `UNSURE` (2).
    #[must_use]
    pub const fn is_void(&self) -> i32 {
        self.is_void
    }
    /// Set the native void flag.
    pub fn set_is_void(&mut self, value: i32) {
        self.is_void = value;
    }
    /// Owning B-rep, or NULL when the region is detached (`GetBrep`).
    #[must_use]
    pub const fn get_brep<'a>(&self, brep: &'a Brep) -> Option<&'a Brep> {
        if self.in_brep {
            Some(brep)
        } else {
            None
        }
    }

    /// Whether this is the owning B-rep's designated infinite region.
    #[must_use]
    pub fn is_infinite_region(&self, brep: &Brep, id: RegionId) -> bool {
        self.in_brep && brep.infinite_region() == Some(id)
    }
    /// Region shell list, retaining optional attribute filtering via the caller's
    /// actual attribute lookup rather than dropping or defaulting that parameter.
    ///
    /// # Errors
    /// Those of the attribute lookup.
    pub fn get_shells<F>(
        &self,
        attribute_id: Option<u64>,
        mut has_attribute: F,
    ) -> Result<Vec<ShellId>>
    where
        F: FnMut(ShellId, u64) -> Result<bool>,
    {
        let mut output = Vec::new();
        for &shell in &self.shells {
            if let Some(attribute) = attribute_id {
                if !has_attribute(shell, attribute)? {
                    continue;
                }
            }
            output.push(shell);
        }
        Ok(output)
    }
}

/// Select the first shell according to `SmRegion::OrderShells`, rotating the
/// circular head while preserving every other link's native relative order.
///
/// # Errors
/// Invalid arena references or an unported inner-shell classification.
pub fn order_region_shells(kernel: &mut Kernel, brep: &mut Brep, id: RegionId) -> Result<()> {
    let shells = brep.region(id)?.shells.clone();
    if shells.len() < 2 {
        return Ok(());
    }
    let first = shells.first().copied().ok_or(invalid_members())?;
    if brep.shell(first)?.is_faceuse_shell() {
        return Ok(());
    }
    let mut faceuse_shells = Vec::new();
    for &shell in &shells {
        if brep.shell(shell)?.is_faceuse_shell() {
            faceuse_shells.push(shell);
        }
    }
    let mut chosen = faceuse_shells.first().copied();
    if faceuse_shells.len() > 1 {
        for &shell in &faceuse_shells {
            if brep.shell(shell)?.is_closed(brep, shell)? {
                match brep.shell(shell)?.is_inner_shell(kernel, brep) {
                    Ok(true) => {
                        chosen = Some(shell);
                        break;
                    }
                    Err(error) if error.is_unported() => return Err(error),
                    Ok(false) | Err(_) => {}
                }
            }
        }
    }
    if let Some(chosen) = chosen {
        let position = shells
            .iter()
            .position(|&shell| shell == chosen)
            .ok_or(invalid_members())?;
        brep.region_mut(id)?.shells.rotate_left(position);
    }
    Ok(())
}

/// Wire edges of a shell, preserving native stack and first-seen order. A wire
/// shell claims and locks a context mark (`SmNewMarkAndLock`, all marks) for its
/// traversal, so the context's mark counters advance as upstream's do.
///
/// # Errors
/// Dangling topology links, native undefined pointer accesses, or the missing
/// vertex edge-list traversal; the mark is released on every path. There is no
/// recursion.
pub fn wire_edges(kernel: &mut Kernel, brep: &Brep, id: ShellId) -> Result<Vec<EdgeId>> {
    const WIRE_OP: &str = "topology::shell::wire_edges";
    let shell = brep.shell(id)?;
    let mut output = Vec::new();
    if shell.is_vertex_shell() {
        return Ok(output);
    }
    if shell.is_wire_shell() {
        with_mark_lock(kernel, ALL_MARKS, |kernel, mark| {
            let first = shell.wire_edge(brep)?.ok_or(invalid_members())?;
            let mut marks = Marks::default();
            let mut stack = vec![first];
            while let Some(edge) = stack.pop() {
                for vertex in crate::topology::edge::vertices(brep, edge, None)? {
                    // Upstream marks every listed vertex: a NULL start vertex is a
                    // NULL dereference.
                    let vertex = vertex.ok_or_else(|| SolidError::null_pointer(WIRE_OP))?;
                    if marks.is_marked(kernel, Entity::Vertex(vertex), mark) {
                        continue;
                    }
                    marks.mark(kernel, Entity::Vertex(vertex), mark);
                    for adjacent in crate::topology::vertex::edges(brep, vertex, None)? {
                        if marks.is_marked(kernel, Entity::Edge(adjacent), mark) {
                            continue;
                        }
                        marks.mark(kernel, Entity::Edge(adjacent), mark);
                        output.push(adjacent);
                        stack.push(adjacent);
                    }
                }
            }
            Ok(())
        })??;
        return Ok(output);
    }
    // GetBrep is required even when the edge list happens to be empty.
    let owner = shell_brep(brep, id)?.ok_or(invalid_members())?;
    // SmBrep::GetEdges (unit topology.brep_p1, which replaced the co-port)
    for edge in crate::topology::brep::p1::edges(owner, None)? {
        if crate::topology::edge::is_wire(owner, edge)? {
            let primary = crate::topology::edgeuse::p1::primary_edgeuse(owner, edge)?
                .ok_or(invalid_members())?;
            if owner.edgeuse(primary)?.owner == crate::topology::EdgeuseOwner::Shell(Some(id)) {
                output.push(edge);
            }
        }
    }
    Ok(output)
}

/// `pRegion->Remove(pShell)` (`SmOwningTopology::Remove`, inline in
/// `SmOwningTopology.h`): takes the shell off the region's list and clears its
/// owner, leaving every other shell's relative order.
///
/// # Errors
/// Native `SM_ERR` (as `InvalidInput`) when the shell is not in this region's list,
/// and dangling ids.
pub fn region_remove_shell(brep: &mut Brep, region: RegionId, shell: ShellId) -> Result<()> {
    if brep.shell(shell)?.region != Some(region) {
        return Err(invalid_members());
    }
    let list = &mut brep.region_mut(region)?.shells;
    let index = list
        .iter()
        .position(|id| *id == shell)
        .ok_or(invalid_members())?;
    list.remove(index);
    brep.shell_mut(shell)?.region = None;
    Ok(())
}

/// `pRegion->PostInsert(pShell)` (`SmOwningTopology::PostInsert`): appends the shell
/// to the tail of the region's list and makes the region its owner.
///
/// # Errors
/// `InvalidInput` when a region's list still holds the shell (this one's, or that of
/// the region it names), and dangling ids.
pub fn region_post_insert_shell(brep: &mut Brep, region: RegionId, shell: ShellId) -> Result<()> {
    let owner = brep.shell(shell)?.region;
    if brep.region(region)?.shells.contains(&shell) {
        return Err(invalid_members());
    }
    if let Some(owner) = owner {
        if brep.region(owner)?.shells.contains(&shell) {
            return Err(invalid_members());
        }
    }
    brep.region_mut(region)?.shells.push(shell);
    brep.shell_mut(shell)?.region = Some(region);
    Ok(())
}

/// `pShell->Remove(pFU)` (`SmOwningTopology::Remove`): takes the face use off the
/// shell's face-use list, clears its owner and links, and closes the list over the
/// gap (the head moves to the next member when it was the head; a list left
/// without members gets a NULL head).
///
/// # Errors
/// [`SolidError::sm_err`] (upstream's `SM_ERR` return) when the face use's owner is
/// not this shell or it is missing from the list; `InvalidInput` for a shell whose
/// head is not a face use; dangling ids.
pub fn shell_remove_faceuse(brep: &mut Brep, shell: ShellId, faceuse: FaceuseId) -> Result<()> {
    const OP: &str = "topology::shell::shell_remove_faceuse";
    if brep.faceuse(faceuse)?.shell != Some(shell) {
        return Err(SolidError::sm_err(OP));
    }
    let members = &mut brep.shell_mut(shell)?.members;
    match members {
        ShellMembers::Faceuses(list) => {
            let index = list
                .iter()
                .position(|id| *id == faceuse)
                .ok_or_else(|| SolidError::sm_err(OP))?;
            list.remove(index);
            if list.is_empty() {
                *members = ShellMembers::Empty;
            }
        }
        ShellMembers::Empty => return Err(SolidError::sm_err(OP)),
        _ => return Err(invalid_members()),
    }
    let record = brep.faceuse_mut(faceuse)?;
    record.shell = None;
    record.sibling_links = None;
    refresh_faceuse_sibling_links(brep, shell)
}

/// `pShell->PostInsert(pFU)` (`SmOwningTopology::PostInsert`): appends the face use
/// to the tail of the shell's face-use list and makes the shell its owner. The
/// shell type is left as it is, as upstream.
///
/// # Errors
/// `InvalidInput` when a shell's face-use list still holds the face use, or when the
/// shell's head is a wire or vertex use; dangling ids.
pub fn shell_post_insert_faceuse(
    brep: &mut Brep,
    shell: ShellId,
    faceuse: FaceuseId,
) -> Result<()> {
    if let Some(owner) = brep.faceuse(faceuse)?.shell {
        if let ShellMembers::Faceuses(list) = &brep.shell(owner)?.members {
            if list.contains(&faceuse) {
                return Err(invalid_members());
            }
        }
    }
    let target = &mut brep.shell_mut(shell)?.members;
    match target {
        ShellMembers::Empty => *target = ShellMembers::Faceuses(vec![faceuse]),
        ShellMembers::Faceuses(list) => list.push(faceuse),
        _ => return Err(invalid_members()),
    }
    brep.faceuse_mut(faceuse)?.shell = Some(shell);
    refresh_faceuse_sibling_links(brep, shell)
}

/// Move a shell's vertex, wire edges and face uses to another shell.
///
/// `wire_candidates` is the native optional list of B-rep wire edges. When it
/// is present, ownership filters it in caller order; otherwise `GetWireEdges`
/// gathers the source's wires. It is never replaced by an empty default list.
/// Nullable list entries are only rejected if the native branch reads them.
///
/// # Errors
/// Native `SM_ERR` for an illegal vertex-shell move, dangling graph links, or
/// incompatible native list classes. The caller must supply connected topology.
pub fn transfer_children(
    kernel: &mut Kernel,
    brep: &mut Brep,
    source: ShellId,
    recipient: ShellId,
    wire_candidates: Option<&[Option<EdgeId>]>,
) -> Result<()> {
    use crate::topology::edgeuse::p1;
    use crate::topology::{EdgeuseOwner, VertexuseOwner};
    const OP: &str = "topology::shell::transfer_children";
    brep.shell(source)?;
    if shell_list_size(brep, recipient)? == 0 {
        brep.shell_mut(recipient)?.shell_type = ShellType::Unknown;
        if brep.shell(source)?.is_vertex_shell() {
            let ShellMembers::Vertexuse(vertexuse) = brep.shell(source)?.members else {
                return Err(invalid_members());
            };
            brep.vertexuse(vertexuse)?;
            let target = brep.shell_mut(recipient)?;
            target.shell_type = ShellType::Vertex;
            target.members = ShellMembers::Vertexuse(vertexuse);
            brep.vertexuse_mut(vertexuse)?.owner = VertexuseOwner::Shell(Some(recipient));
            clear_shell_members(brep, source)?;
            return Ok(());
        }
    } else {
        if brep.shell(recipient)?.is_vertex_shell() {
            return if shell_list_size(brep, source)? == 0 {
                Ok(())
            } else {
                Err(SolidError::sm_err(OP))
            };
        }
        if brep.shell(source)?.is_vertex_shell() {
            return Err(SolidError::sm_err(OP));
        }
    }

    let wires = if let Some(candidates) = wire_candidates {
        let mut wires = Vec::new();
        for &edge in candidates {
            let edge = edge.ok_or(invalid_members())?;
            let primary = p1::primary_edgeuse(brep, edge)?.ok_or(invalid_members())?;
            if brep.edgeuse(primary)?.owner == EdgeuseOwner::Shell(Some(source)) {
                wires.push(edge);
            }
        }
        wires
    } else {
        wire_edges(kernel, brep, source)?
    };
    if let Some(&first) = wires.first() {
        if brep.shell(recipient)?.is_unknown_shell() {
            let primary = p1::primary_edgeuse(brep, first)?.ok_or(invalid_members())?;
            let target = brep.shell_mut(recipient)?;
            target.members = ShellMembers::Edgeuse(primary);
            target.shell_type = ShellType::Wire;
            // UpdateListSize counts this edge's radial list; the shared arena
            // model retains that list on Edge::edgeuses rather than separately.
        }
        for edge in wires {
            // SmEdge::GetEdgeuses with neither optional filter: the native
            // circular radial list order.
            let edgeuses = crate::topology::edge::edgeuses(brep, edge, None, None)?;
            for edgeuse in edgeuses {
                brep.edgeuse_mut(edgeuse)?.owner = EdgeuseOwner::Shell(Some(recipient));
            }
        }
    }

    let faceuses = brep.shell(source)?.get_faceuses()?;
    if !faceuses.is_empty() {
        if !brep.shell(recipient)?.is_faceuse_shell() {
            let target = brep.shell_mut(recipient)?;
            target.shell_type = ShellType::Faceuse;
            target.members = ShellMembers::Empty;
        }
        for faceuse in faceuses {
            if brep.faceuse(faceuse)?.shell != Some(source) {
                return Err(invalid_members());
            }
            remove_and_post_insert(brep, faceuse, source, recipient)?;
        }
        // Self-transfer clears the shell head but leaves this face-use cycle
        // connected. Preserve those native links independently of the head.
        refresh_faceuse_sibling_links(brep, recipient)?;
    }
    clear_shell_members(brep, source)
}

// `source->Remove(faceuse)` then `recipient->PostInsert(faceuse)`
// (`SmOwningTopology`, inline in `SmOwningTopology.h`): the member moves to the
// recipient's tail without changing either list's remaining relative order; a
// list left without members gets a NULL head. Sibling links are not refreshed.
fn remove_and_post_insert(
    brep: &mut Brep,
    faceuse: FaceuseId,
    source: ShellId,
    recipient: ShellId,
) -> Result<()> {
    let members = &mut brep.shell_mut(source)?.members;
    match members {
        ShellMembers::Faceuses(list) => {
            let index = list
                .iter()
                .position(|id| *id == faceuse)
                .ok_or(invalid_members())?;
            list.remove(index);
            if list.is_empty() {
                *members = ShellMembers::Empty;
            }
        }
        _ => return Err(invalid_members()),
    }
    let target = &mut brep.shell_mut(recipient)?.members;
    match target {
        ShellMembers::Empty => *target = ShellMembers::Faceuses(vec![faceuse]),
        ShellMembers::Faceuses(list) => list.push(faceuse),
        _ => return Err(invalid_members()),
    }
    brep.faceuse_mut(faceuse)?.shell = Some(recipient);
    Ok(())
}

/// Move one face use from its shell to the tail of `recipient`
/// (`pOldShell->Remove(pFU); pNewShell->PostInsert(pFU)` with
/// `pOldShell = pFU->GetShell()`, as `SmTopologyTraverser::FaceuseShellReplacement`
/// does). Both shells' face-use sibling links are refreshed. The caller skips a
/// face use already in `recipient`.
///
/// # Errors
/// A face use without a shell (a NULL dereference upstream), a face use missing
/// from its shell's face-use list, a recipient holding wire/vertex members, or
/// dangling ids.
pub fn move_faceuse(brep: &mut Brep, faceuse: FaceuseId, recipient: ShellId) -> Result<()> {
    let source = brep
        .faceuse(faceuse)?
        .shell
        .ok_or_else(|| SolidError::null_pointer("topology::shell::move_faceuse"))?;
    brep.shell(recipient)?;
    remove_and_post_insert(brep, faceuse, source, recipient)?;
    refresh_faceuse_sibling_links(brep, source)?;
    refresh_faceuse_sibling_links(brep, recipient)
}

/// `pShell->PostInsert(pFU)` (`SmOwningTopology::PostInsert`, inline in
/// `SmOwningTopology.h`) of a face use not on any list: it joins the tail of the
/// shell's face-use list (a NULL head makes it the head) and gets the shell as owner.
/// Literal sibling links of the members, when any are held, are refreshed.
///
/// # Errors
/// A shell holding wire/vertex members, or dangling ids.
pub fn post_insert_faceuse(brep: &mut Brep, shell: ShellId, faceuse: FaceuseId) -> Result<()> {
    brep.faceuse(faceuse)?;
    let members = &mut brep.shell_mut(shell)?.members;
    match members {
        ShellMembers::Empty => *members = ShellMembers::Faceuses(vec![faceuse]),
        ShellMembers::Faceuses(list) => list.push(faceuse),
        _ => return Err(invalid_members()),
    }
    brep.faceuse_mut(faceuse)?.shell = Some(shell);
    let mut linked = false;
    for member in brep.shell(shell)?.get_faceuses()? {
        linked |= member != faceuse && brep.faceuse(member)?.sibling_links.is_some();
    }
    if linked {
        refresh_faceuse_sibling_links(brep, shell)?;
    }
    Ok(())
}

fn clear_shell_members(brep: &mut Brep, id: ShellId) -> Result<()> {
    let shell = brep.shell_mut(id)?;
    shell.shell_type = ShellType::Unknown;
    shell.members = ShellMembers::Empty;
    Ok(())
}

/// `SmShell::GetSize`: the shell's `m_lListSize`, recomputed from its typed members
/// (`UpdateListSize`; a wire shell counts its head edge's radial edge uses).
///
/// # Errors
/// A wire head without an edge, and dangling ids.
pub fn shell_list_size(brep: &Brep, id: ShellId) -> Result<usize> {
    match &brep.shell(id)?.members {
        ShellMembers::Empty => Ok(0),
        ShellMembers::Faceuses(members) => Ok(members.len()),
        ShellMembers::Vertexuse(_) => Ok(1),
        ShellMembers::Edgeuse(id) => {
            let edge = brep.edgeuse(*id)?.edge.ok_or(invalid_members())?;
            Ok(brep.edge(edge)?.edgeuses.len())
        }
    }
}

/// Native next/previous face-use links, independently of a shell's list head.
pub type FaceuseSiblingLinks = (Option<FaceuseId>, Option<FaceuseId>);

/// Typed adapter for a face use's native `GetNext` and `GetLast` links.
///
/// # Errors
/// A dangling id or a constructed owner vector that does not contain its member.
pub fn faceuse_sibling_links(brep: &Brep, id: FaceuseId) -> Result<FaceuseSiblingLinks> {
    let faceuse = brep.faceuse(id)?;
    if let Some(links) = faceuse.sibling_links {
        return Ok(links);
    }
    let Some(owner) = faceuse.shell else {
        return Ok((None, None));
    };
    let ShellMembers::Faceuses(members) = &brep.shell(owner)?.members else {
        return Err(invalid_members());
    };
    let mut previous = members.last().copied();
    for (&member, &next) in members.iter().zip(members.iter().cycle().skip(1)) {
        if member == id {
            return Ok((Some(next), previous));
        }
        previous = Some(member);
    }
    Err(invalid_members())
}

fn refresh_faceuse_sibling_links(brep: &mut Brep, owner: ShellId) -> Result<()> {
    let members = brep.shell(owner)?.get_faceuses()?;
    let mut previous = members.last().copied();
    for (&member, &next) in members.iter().zip(members.iter().cycle().skip(1)) {
        brep.faceuse_mut(member)?.sibling_links = Some((Some(next), previous));
        previous = Some(member);
    }
    Ok(())
}

/// Whether a shell fits within `tolerance` (`SmShell::IsDegenerate`).
/// `None` retains the native default `EFF_ZERO`, independently of zone tolerance.
///
/// # Errors
/// Invalid graph references, an undefined native pointer access, or an unported
/// bounding-box dependency. Ordinary native box errors retain the partial box.
pub fn shell_is_degenerate(
    kernel: &mut Kernel,
    brep: &Brep,
    id: ShellId,
    tolerance: Option<f64>,
) -> Result<bool> {
    if brep.shell(id)?.is_vertex_shell() {
        return Ok(true);
    }
    let bounds = shell_bounds_for_degeneracy(kernel, brep, id)?;
    // Native IsDegenerate deliberately ignores CalculateBoundingBox's status.
    Ok(bounds.is_point_sized(tolerance.unwrap_or(crate::math::EFF_ZERO)))
}

/// Whether a region fits within `tolerance` (`SmRegion::IsDegenerate`).
/// `None` retains the native default `EFF_ZERO`.
///
/// # Errors
/// Invalid graph references or an unported bounding-box dependency. Ordinary
/// native box errors are ignored while retaining the initialized/partial box.
pub fn region_is_degenerate(
    kernel: &mut Kernel,
    brep: &Brep,
    id: RegionId,
    tolerance: Option<f64>,
) -> Result<bool> {
    let bounds = region_bounds_for_degeneracy(kernel, brep, id)?;
    Ok(bounds.is_point_sized(tolerance.unwrap_or(crate::math::EFF_ZERO)))
}

// SmRegion/SmShell::CalculateBoundingBox (unit query.face_p1) with the default
// bTight = FALSE. The IsDegenerate callers ignore a native failure status and read
// the partial union of the children before the failing one; missing dependencies
// and unsafe native graph accesses still propagate.
fn region_bounds_for_degeneracy(
    kernel: &mut Kernel,
    brep: &Brep,
    id: RegionId,
) -> Result<crate::geom::extent3d::Extent3d> {
    let mut bounds = crate::geom::extent3d::Extent3d::new();
    let status = crate::query::face::p1::calculate_region_bounding_box_into(
        kernel,
        brep,
        id,
        false,
        &mut bounds,
    );
    native_bounds(bounds, status)
}

fn shell_bounds_for_degeneracy(
    kernel: &mut Kernel,
    brep: &Brep,
    id: ShellId,
) -> Result<crate::geom::extent3d::Extent3d> {
    let mut bounds = crate::geom::extent3d::Extent3d::new();
    let status = crate::query::face::p1::calculate_shell_bounding_box_into(
        kernel,
        brep,
        id,
        false,
        &mut bounds,
    );
    native_bounds(bounds, status)
}

fn native_bounds(
    bounds: crate::geom::extent3d::Extent3d,
    status: Result<()>,
) -> Result<crate::geom::extent3d::Extent3d> {
    match status {
        // Native SER returns the box unioned so far.
        Ok(()) | Err(SolidError::Upstream { .. } | SolidError::Nlib { .. }) => Ok(bounds),
        // Missing dependencies and safety errors must not become ordinary native
        // status results or disappear into an initialized extent.
        Err(error) => Err(error),
    }
}

/// Outputs of a shell ray query, including writes made before an error.
#[derive(Debug, Clone)]
pub struct ShellRayState<O> {
    /// Region containing the ray start, or no region after a missed shell.
    pub region: Option<RegionId>,
    /// Classification of the selected intersection.
    pub classification: crate::intersect::point_class::PointClassification<O>,
}

/// Caller parameters and initial output values for `SmShell::RayFire`.
pub struct ShellRayInput<O> {
    /// Ray start.
    pub start: crate::math::Point3,
    /// Caller direction, before native sizing and normalization.
    pub direction: crate::math::Vec3,
    /// Intersection search tolerance.
    pub zone_tolerance: f64,
    /// Optional radius, bypassing B-rep bounding-box construction.
    pub sphere_bound: Option<f64>,
    /// Initial output values, retained on paths that do not write them.
    pub state: ShellRayState<O>,
}

/// Status and observable output state of a ray query.
pub struct ShellRayAttempt<O> {
    /// Native operation status or an explicit missing dependency.
    pub status: Result<()>,
    /// Outputs after all writes preceding that status.
    pub state: ShellRayState<O>,
}

/// Borrowed inputs for the B-rep's local sector classification.
pub struct ShellRaySector<'a, O> {
    /// Actual owner arena key.
    pub brep: crate::topology::BrepId,
    /// Actual line segment from ray start to intersection.
    pub curve: &'a crate::nurbs::b_spline_curve::BSplineCurve,
    /// Natural interval of that curve.
    pub interval: crate::geom::extent1d::Extent1d,
    /// Native opposite orientation for the incoming segment.
    pub orientation: crate::nurbs::b_spline_surface_n_lib::p2::OrientType,
    /// Classification produced by the selected intersection.
    pub classification: &'a crate::intersect::point_class::PointClassification<O>,
}

/// Sector result retaining a possible region write even on error.
pub struct ShellRaySectorAttempt {
    /// Native sector status.
    pub status: Result<()>,
    /// `None` leaves the region unchanged; `Some(None)` writes NULL.
    pub region_update: Option<Option<RegionId>>,
}

/// Actual geometry and owner context needed by shell ray queries.
/// No global owner registry or duplicate geometry metadata is introduced.
pub trait ShellRayWorld<O>: crate::intersect::topology_solver::p1::TopologyObjects<O> {
    /// The shape of the shell's boundary faces (`SmShape(sBreps, sFaces, ...)`) of
    /// `brep`. Worlds that cannot build the real shape (a harness without the B-rep's
    /// cached geometry) defer here instead of failing later.
    fn shell_ray_shape(
        &self,
        kernel: &Kernel,
        brep: (crate::topology::BrepId, &Brep),
        faces: &[crate::geom::object::FaceKey],
    ) -> Result<Shape> {
        Shape::new(kernel, &brep, &[], faces, &[], &[])
    }

    /// The topology entity behind a handle the shape solve returned (a vertex, edge
    /// or face of the B-rep), as `LocalCurveSector` reads its classification.
    fn class_entity(&self, object: O) -> Option<crate::EntityRef>;

    /// Classify the incoming line segment at its known intersection
    /// (`pBrep->LocalCurveSector(curve, ivl, orientation, sPC, pRegion)`, unit
    /// `topology.brep_p2`): [`crate::topology::brep::p2::local_curve_sector`] on `brep`
    /// (the sectoring may add missing UV trim curves, as upstream's `const` method does),
    /// the classification's objects mapped by [`ShellRayWorld::class_entity`]. A failure
    /// leaves the region unchanged.
    fn shell_ray_sector(
        &self,
        kernel: &Kernel,
        brep: &mut Brep,
        request: ShellRaySector<'_, O>,
    ) -> ShellRaySectorAttempt
    where
        O: crate::intersect::point_class::ClassObject,
    {
        let sector = (|| {
            let hit = request.classification.map_objects(&mut |o| {
                self.class_entity(o).ok_or(SolidError::invalid_input(
                    "topology::shell::ShellRayWorld::shell_ray_sector",
                    "intersection without a topology entity",
                ))
            })?;
            let curve = crate::curve::curve::Curve::from(request.curve.clone());
            crate::topology::brep::p2::local_curve_sector(
                kernel,
                brep,
                &curve,
                &request.interval,
                request.orientation,
                &hit,
            )
        })();
        match sector {
            Ok(region) => ShellRaySectorAttempt {
                status: Ok(()),
                region_update: Some(region),
            },
            Err(e) => ShellRaySectorAttempt {
                status: Err(e),
                region_update: None,
            },
        }
    }
}

/// Fire a sized ray at this shell's distinct boundary faces (`SmShell::RayFire`).
/// Native partial output writes survive an error in the returned attempt.
/// Missing geometry queries are explicit dependency errors.
pub fn ray_fire<O, W>(
    kernel: &mut Kernel,
    brep: &mut Brep,
    brep_id: crate::topology::BrepId,
    shell_id: ShellId,
    world: &W,
    input: ShellRayInput<O>,
) -> ShellRayAttempt<O>
where
    O: crate::geom::solution_array::SolutionObject + crate::intersect::point_class::ClassObject,
    W: ShellRayWorld<O>,
{
    use crate::geom::global_solver::{SolutionRequested, SolverOperation};
    use crate::intersect::point_class::PointClassType;
    use crate::intersect::topology_solver::{p1::TopologySolver, p2::TopologySolveOptions};
    use crate::math::{BIG_DOUBLE, EFF_ZERO, EFF_ZERO_SQRT};
    use crate::nurbs::b_spline_curve::BSplineCurve;
    use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
    const OP: &str = "topology::shell::ray_fire";
    let mut state = input.state;
    let status = (|| {
        let radius = if let Some(radius) = input.sphere_bound {
            radius
        } else {
            shell_brep(brep, shell_id)?.ok_or(SolidError::null_pointer(OP))?;
            // pBrep->CalculateBoundingBox(sBBox) (unit `query.brep`, default arguments)
            qbrep::calculate_bounding_box(kernel, brep, BoundingBoxOptions::default())?
                .sphere_bound()
                .radius
        };
        let end = input.start + input.direction * (3.01 * radius);
        let direction = (end - input.start)
            .unitize()
            .map_err(|_| SolidError::Upstream {
                operation: OP,
                code: 1007, // SmVector3d::Unitize returns SM_ERR_INVALID_INPUT.
                entity: None,
            })?;
        // `SmNewMarkAndLock` with the default (all marks) mask, released on every path.
        let faces = with_mark_lock(kernel, ALL_MARKS, |kernel, mark| {
            let mut marks = Marks::default();
            let mut faces = Vec::new();
            for id in brep.shell(shell_id)?.get_faceuses()? {
                let faceuse = brep.faceuse(id)?;
                let face = faceuse.face.ok_or(SolidError::null_pointer(OP))?;
                if marks.is_marked(kernel, Entity::Face(face), mark) {
                    continue;
                }
                marks.mark(kernel, Entity::Face(face), mark);
                let mate = brep.faceuse(faceuse.mate.ok_or(SolidError::null_pointer(OP))?)?;
                if mate.shell != Some(shell_id) {
                    faces.push((brep_id, face));
                }
            }
            Ok::<_, SolidError>(faces)
        })??;
        let shape = world.shell_ray_shape(kernel, (brep_id, brep), &faces)?;
        let vectors = [direction];
        let solutions = TopologySolver::shape_point_solve(
            &*kernel,
            world,
            &shape,
            input.start,
            TopologySolveOptions {
                operation: SolverOperation::Rayfire,
                requested: SolutionRequested::All,
                tolerance_3d: input.zone_tolerance,
                best_answer_so_far_sq: BIG_DOUBLE,
                opt_vectors: &vectors,
            },
        )?;
        let mut iter = solutions.as_slice().iter();
        let Some(mut chosen) = iter.next() else {
            state.region = None;
            state
                .classification
                .set_class_object(PointClassType::Unknown, None);
            return Ok(());
        };
        // Upstream compares every candidate against the INITIAL ray parameter,
        // even after selecting a different solution.
        let initial_parameter = chosen.start.solution_value;
        for candidate in iter {
            if candidate.start.solution_value - initial_parameter < EFF_ZERO_SQRT {
                if candidate.num_variables < chosen.num_variables {
                    chosen = candidate;
                    continue;
                }
                if candidate.num_variables > chosen.num_variables {
                    continue;
                }
            }
            if candidate.start.param(0)? < chosen.start.param(0)? {
                chosen = candidate;
            }
        }
        let parameter = chosen.start.solution_value;
        if parameter < -EFF_ZERO {
            state.region = None;
            state
                .classification
                .set_class_object(PointClassType::Unknown, None);
        }
        if parameter < EFF_ZERO * (1.0 + input.start.max_dimension()) {
            return Err(SolidError::sm_err(OP));
        }
        let object = chosen
            .objects
            .first()
            .copied()
            .flatten()
            .ok_or(SolidError::null_pointer(OP))?;
        state.classification.load_from_intersection(
            input.zone_tolerance,
            object,
            &chosen.start,
            0,
            parameter,
        )?;
        let endpoint = input.start + direction * parameter;
        shell_brep(brep, shell_id)?.ok_or(SolidError::null_pointer(OP))?;
        let curve = BSplineCurve::create_line_segment(3, input.start, endpoint)?;
        let sector = world.shell_ray_sector(
            &*kernel,
            brep,
            ShellRaySector {
                brep: brep_id,
                curve: &curve,
                interval: curve.natural_interval(),
                orientation: OrientType::Opposite,
                classification: &state.classification,
            },
        );
        if let Some(region) = sector.region_update {
            state.region = region;
        }
        sector.status
    })();
    ShellRayAttempt { status, state }
}

/// A region resolved to its actual typed arena and B-rep identity.
/// The caller supplies the identity of that arena; detached regions retain no
/// owning B-rep even though their records still need storage.
#[derive(Clone, Copy, Debug)]
pub struct RegionRef<'a> {
    /// Arena containing the live region record.
    pub brep: &'a Brep,
    /// Identity of that B-rep in the caller's topology context.
    pub brep_id: crate::topology::BrepId,
    /// Actual region key in the arena.
    pub region_id: RegionId,
}

impl RegionRef<'_> {
    /// Resolve the native owner, including NULL for a detached region.
    ///
    /// # Errors
    /// A dangling region key.
    pub fn owning_brep(&self) -> Result<Option<crate::topology::BrepId>> {
        Ok(self
            .brep
            .region(self.region_id)?
            .get_brep(self.brep)
            .map(|_| self.brep_id))
    }

    /// The raw native void flag, including UNSURE (2).
    ///
    /// # Errors
    /// A dangling region key.
    pub fn void_state(&self) -> Result<i32> {
        Ok(self.brep.region(self.region_id)?.is_void())
    }

    /// Whether this is its owner's infinite region.
    ///
    /// # Errors
    /// A dangling region key.
    pub fn is_infinite(&self) -> Result<bool> {
        Ok(self
            .brep
            .region(self.region_id)?
            .is_infinite_region(self.brep, self.region_id))
    }
}

/// Borrowed resolution of opaque attribute object keys to live typed regions.
/// Implementations must preserve actual B-rep identity across arenas; this
/// interface stores no duplicate region flags and introduces no global registry.
pub trait RegionLookup {
    /// Resolve the actual region record for this object.
    ///
    /// # Errors
    /// An unknown object, wrong object class or dangling topology key.
    fn resolve_region(&self, object: crate::geom::attribute::ObjectKey) -> Result<RegionRef<'_>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::Faceuse;

    #[test]
    fn region_order_rotates_the_circular_head_without_moving_other_links() {
        let mut brep = Brep::new();
        let ids = [
            ShellType::Wire,
            ShellType::Unknown,
            ShellType::Faceuse,
            ShellType::Vertex,
        ]
        .into_iter()
        .map(|shell_type| {
            brep.add_shell_record(Shell {
                shell_type,
                ..Shell::default()
            })
            .unwrap()
        })
        .collect::<Vec<_>>();
        let region = brep
            .add_region_record(Region {
                shells: ids.clone(),
                ..Region::default()
            })
            .unwrap();
        order_region_shells(&mut Kernel::default(), &mut brep, region).unwrap();
        assert_eq!(
            brep.region(region).unwrap().shells,
            vec![ids[2], ids[3], ids[0], ids[1]]
        );
    }

    #[test]
    fn mated_shells_keep_first_seen_order_when_a_shell_reappears() {
        let mut brep = Brep::new();
        let source = brep.add_shell_record(Shell::default()).unwrap();
        let a = brep.add_shell_record(Shell::default()).unwrap();
        let b = brep.add_shell_record(Shell::default()).unwrap();
        let mut sources = Vec::new();
        for target in [b, a, b] {
            let mate = brep
                .add_faceuse_record(Faceuse {
                    shell: Some(target),
                    ..Faceuse::default()
                })
                .unwrap();
            sources.push(
                brep.add_faceuse_record(Faceuse {
                    shell: Some(source),
                    mate: Some(mate),
                    ..Faceuse::default()
                })
                .unwrap(),
            );
        }
        let shell = brep.shell_mut(source).unwrap();
        shell.shell_type = ShellType::Faceuse;
        shell.members = ShellMembers::Faceuses(sources);
        assert_eq!(
            brep.shell(source)
                .unwrap()
                .mated_shells(&mut Kernel::default(), &brep)
                .unwrap(),
            vec![b, a]
        );
        assert!(brep
            .shell(source)
            .unwrap()
            .is_closed(&brep, source)
            .unwrap());
    }

    #[test]
    fn changing_shell_type_preserves_the_list_head_and_members() {
        let members = vec![FaceuseId::from_index(2), FaceuseId::from_index(0)];
        let mut shell = Shell {
            shell_type: ShellType::Faceuse,
            members: ShellMembers::Faceuses(members.clone()),
            ..Shell::default()
        };
        shell.set_shell_type(16999);
        assert!(shell.get_faceuses().unwrap().is_empty());
        shell.set_shell_type(16022);
        assert_eq!(shell.get_faceuses().unwrap(), members);
    }

    #[test]
    fn move_faceuse_removes_then_post_inserts_and_relinks_siblings() {
        let mut brep = Brep::new();
        let from = brep.add_shell_record(Shell::default()).unwrap();
        let to = brep.add_shell_record(Shell::default()).unwrap();
        let mut ids = Vec::new();
        for shell in [from, from, to] {
            ids.push(
                brep.add_faceuse_record(Faceuse {
                    shell: Some(shell),
                    ..Faceuse::default()
                })
                .unwrap(),
            );
        }
        let (a, b, c) = (ids[0], ids[1], ids[2]);
        for (shell, list) in [(from, vec![a, b]), (to, vec![c])] {
            let s = brep.shell_mut(shell).unwrap();
            s.shell_type = ShellType::Faceuse;
            s.members = ShellMembers::Faceuses(list);
        }
        // the head moves: the source list keeps its order, the recipient gains a tail
        move_faceuse(&mut brep, a, to).unwrap();
        assert_eq!(brep.shell(from).unwrap().get_faceuses().unwrap(), vec![b]);
        assert_eq!(brep.shell(to).unwrap().get_faceuses().unwrap(), vec![c, a]);
        assert_eq!(brep.faceuse(a).unwrap().shell, Some(to));
        assert_eq!(faceuse_sibling_links(&brep, a).unwrap(), (Some(c), Some(c)));
        assert_eq!(faceuse_sibling_links(&brep, b).unwrap(), (Some(b), Some(b)));
        // the last member leaves a NULL head
        move_faceuse(&mut brep, b, to).unwrap();
        assert_eq!(brep.shell(from).unwrap().members, ShellMembers::Empty);
        assert_eq!(
            brep.shell(to).unwrap().get_faceuses().unwrap(),
            vec![c, a, b]
        );
        // a face use missing from its shell's list is an inconsistent B-rep
        brep.faceuse_mut(c).unwrap().shell = Some(from);
        assert!(move_faceuse(&mut brep, c, to).is_err());
    }

    #[test]
    fn single_member_remove_and_post_insert() {
        let mut brep = Brep::new();
        let s = brep.add_shell_record(Shell::default()).unwrap();
        let t = brep.add_shell_record(Shell::default()).unwrap();
        let mut ids = Vec::new();
        for _ in 0..3 {
            ids.push(
                brep.add_faceuse_record(Faceuse {
                    shell: Some(s),
                    ..Faceuse::default()
                })
                .unwrap(),
            );
        }
        let (a, b, c) = (ids[0], ids[1], ids[2]);
        {
            let sh = brep.shell_mut(s).unwrap();
            sh.shell_type = ShellType::Faceuse;
            sh.members = ShellMembers::Faceuses(vec![a, b, c]);
        }
        assert_eq!(shell_list_size(&brep, s).unwrap(), 3);
        // removing the head moves it to the next member and closes the ring
        shell_remove_faceuse(&mut brep, s, a).unwrap();
        assert_eq!(brep.shell(s).unwrap().get_faceuses().unwrap(), vec![b, c]);
        assert_eq!(brep.faceuse(a).unwrap().shell, None);
        assert_eq!(faceuse_sibling_links(&brep, a).unwrap(), (None, None));
        assert_eq!(faceuse_sibling_links(&brep, b).unwrap(), (Some(c), Some(c)));
        // Remove of a member that is not in the list is upstream's SM_ERR status
        let e = shell_remove_faceuse(&mut brep, s, a).unwrap_err();
        assert!(matches!(e, SolidError::Upstream { .. }));
        // PostInsert into an empty shell makes its head; the type is not touched
        shell_post_insert_faceuse(&mut brep, t, a).unwrap();
        assert_eq!(
            brep.shell(t).unwrap().members,
            ShellMembers::Faceuses(vec![a])
        );
        assert_eq!(brep.shell(t).unwrap().shell_type, ShellType::Unknown);
        assert_eq!(brep.faceuse(a).unwrap().shell, Some(t));
        assert_eq!(faceuse_sibling_links(&brep, a).unwrap(), (Some(a), Some(a)));
        // a member still listed elsewhere is refused
        assert!(shell_post_insert_faceuse(&mut brep, t, b).is_err());
        // the last member leaves a NULL head
        shell_remove_faceuse(&mut brep, s, b).unwrap();
        shell_remove_faceuse(&mut brep, s, c).unwrap();
        assert_eq!(brep.shell(s).unwrap().members, ShellMembers::Empty);
        assert_eq!(shell_list_size(&brep, s).unwrap(), 0);
    }

    #[test]
    fn region_post_insert_appends_and_owns() {
        let mut brep = Brep::new();
        let r = brep.add_region_record(Region::default()).unwrap();
        let q = brep.add_region_record(Region::default()).unwrap();
        let s1 = brep.add_shell_record(Shell::default()).unwrap();
        let s2 = brep.add_shell_record(Shell::default()).unwrap();
        region_post_insert_shell(&mut brep, r, s1).unwrap();
        region_post_insert_shell(&mut brep, r, s2).unwrap();
        assert_eq!(brep.region(r).unwrap().shells, vec![s1, s2]);
        assert_eq!(brep.shell(s2).unwrap().region, Some(r));
        // listed twice, or still in another region's list: refused
        assert!(region_post_insert_shell(&mut brep, r, s1).is_err());
        assert!(region_post_insert_shell(&mut brep, q, s1).is_err());
        region_remove_shell(&mut brep, r, s1).unwrap();
        region_post_insert_shell(&mut brep, q, s1).unwrap();
        assert_eq!(brep.region(r).unwrap().shells, vec![s2]);
        assert_eq!(brep.region(q).unwrap().shells, vec![s1]);
    }
}
