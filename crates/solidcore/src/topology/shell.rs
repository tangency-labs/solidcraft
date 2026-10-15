//! Shell and region topology, with typed arena links and native traversal order.
//!
//! Upstream: `source/SMLib/inc/SmShell.h`, `source/SMLib/src/SmShell.cpp`,
//! `source/SMLib/inc/SmRegion.h`, `source/SMLib/src/SmRegion.cpp`,
//! and the minimum default-argument branches of `source/SMLib/src/SmEdge.cpp`
//! and `source/SMLib/src/SmVertex.cpp` used by wire-edge traversal;
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
//! | [`shell_is_degenerate`], [`region_is_degenerate`] | `IsDegenerate` (native default tolerance) |
//! | `shell_bounds_for_degeneracy`, `region_bounds_for_degeneracy` | minimum default-tight `CalculateBoundingBox` branches (`query.face_p1`) |
//! | [`faceuse_sibling_links`] | typed `SmTopology::GetNext` / `GetLast` for face uses |
//! | `query::edge::edge_vertices`, `topology::vertexuse::edge_is_wire` | `SmEdge::GetVertices` (PR #89), `IsWire` (PR #88) |
//! | `vertex_edges_for_shell` | `SmVertex::GetVertexuses`, `GetEdges` (no face filter) |
//! | [`Region::get_shells`], [`Region::is_void`], [`Region::set_is_void`] | `GetShells`, `IsVoid`, `SetIsVoid` |
//! | [`Region::is_infinite_region`] | `IsInfiniteRegion` |
//! | [`Shell::is_inner_shell`], [`order_region_shells`] | `IsInnerShell`, `OrderShells` |
//!
//! Deviations: circular lists are typed vectors in head-first order, as in the
//! shared Brep arena model. Temporary marks use a local set without changing
//! first-seen order. Incompatible array reinterpret casts return `InvalidInput`;
//! checked member casts retain native NULL. Native
//! notification/logging side effects are not represented by these arena records.
//! Face-use sibling links survive a cleared owner head, as in native self-transfer.
//! There is no recursion. Missing external callees return explicit `Unported` errors.

use crate::error::{Result, SolidError};
use crate::topology::proto_topology::p1::owning_topology;
use crate::topology::{
    Brep, EdgeId, EdgeuseId, FaceuseId, Region, RegionId, Shell, ShellId, VertexId, VertexuseId,
};
use std::collections::BTreeSet;

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
    /// Distinct mate shells in first-seen face-use order.
    ///
    /// # Errors
    /// Missing native mates/owners or dangling arena references.
    pub fn mated_shells(&self, brep: &Brep) -> Result<Vec<ShellId>> {
        let mut output = Vec::new();
        if !self.is_faceuse_shell() {
            return Ok(output);
        }
        let mut seen = BTreeSet::new();
        for faceuse in self.get_faceuses()? {
            let mate = brep.faceuse(faceuse)?.mate.ok_or(invalid_members())?;
            let shell = brep.faceuse(mate)?.shell.ok_or(invalid_members())?;
            brep.shell(shell)?;
            if seen.insert(shell) {
                output.push(shell);
            }
        }
        Ok(output)
    }
    /// Classify whether this is an inner shell through the topology traverser.
    ///
    /// # Errors
    /// Invalid member classes or the unported topology traversal callee.
    pub fn is_inner_shell(&self) -> Result<bool> {
        let _faceuses = self.get_faceuses()?;
        // UNPORTED(topology.topology_traverser): SmTopologyTraverser::IsInnerShell
        Err(SolidError::unported(
            "SmTopologyTraverser::IsInnerShell",
            "topology.topology_traverser",
        ))
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
pub fn order_region_shells(brep: &mut Brep, id: RegionId) -> Result<()> {
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
                match brep.shell(shell)?.is_inner_shell() {
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

/// Wire edges of a shell, preserving native stack and first-seen order.
///
/// # Errors
/// Dangling topology links, native undefined pointer accesses, or the missing
/// vertex edge-list traversal. There is no recursion.
pub fn wire_edges(brep: &Brep, id: ShellId) -> Result<Vec<EdgeId>> {
    let shell = brep.shell(id)?;
    let mut output = Vec::new();
    if shell.is_vertex_shell() {
        return Ok(output);
    }
    if shell.is_wire_shell() {
        let first = shell.wire_edge(brep)?.ok_or(invalid_members())?;
        let mut stack = vec![first];
        let mut seen_vertices = BTreeSet::new();
        let mut seen_edges = BTreeSet::new();
        while let Some(edge) = stack.pop() {
            for vertex in crate::query::edge::edge_vertices(brep, edge)? {
                if !seen_vertices.insert(vertex) {
                    continue;
                }
                for adjacent in vertex_edges_for_shell(brep, vertex)? {
                    if seen_edges.insert(adjacent) {
                        output.push(adjacent);
                        stack.push(adjacent);
                    }
                }
            }
        }
        return Ok(output);
    }
    // The shared arena model represents SmBrep's edge list in insertion order.
    // GetBrep is required even when the edge list happens to be empty.
    let owner = shell_brep(brep, id)?.ok_or(invalid_members())?;
    for (edge, _) in owner.edges().iter() {
        if edge_is_wire_for_shell(owner, edge)? {
            let primary = crate::topology::edgeuse::p1::primary_edgeuse(owner, edge)?
                .ok_or(invalid_members())?;
            if owner.edgeuse(primary)?.owner == crate::topology::EdgeuseOwner::Shell(Some(id)) {
                output.push(edge);
            }
        }
    }
    Ok(output)
}

fn edge_is_wire_for_shell(brep: &Brep, edge: EdgeId) -> Result<bool> {
    crate::topology::vertexuse::edge_is_wire(brep, edge)
}

fn vertex_edges_for_shell(brep: &Brep, vertex: VertexId) -> Result<Vec<EdgeId>> {
    use crate::topology::edgeuse::p1;
    use crate::topology::VertexuseOwner;
    let mut output = Vec::new();
    for &vertexuse in &brep.vertex(vertex)?.vertexuses {
        if let VertexuseOwner::Edgeuse(edgeuse) = brep.vertexuse(vertexuse)?.owner {
            let edgeuse = edgeuse.ok_or(invalid_members())?;
            // Upstream asks for the face even when its optional filter is NULL.
            let _face = p1::face(brep, edgeuse)?;
            let edge = brep.edgeuse(edgeuse)?.edge.ok_or(invalid_members())?;
            if !output.contains(&edge) {
                output.push(edge);
            }
        }
    }
    Ok(output)
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
        wire_edges(brep, source)?
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
            // SmEdge::GetEdgeuses with neither optional filter. The shared edge
            // record already retains its native circular radial list order.
            let edgeuses = brep.edge(edge)?.edgeuses.clone();
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
            // Remove, then PostInsert: the member moves to the recipient's tail
            // without changing either list's remaining relative order.
            match &mut brep.shell_mut(source)?.members {
                ShellMembers::Faceuses(members) => {
                    let index = members
                        .iter()
                        .position(|id| *id == faceuse)
                        .ok_or(invalid_members())?;
                    members.remove(index);
                }
                _ => return Err(invalid_members()),
            }
            let target = &mut brep.shell_mut(recipient)?.members;
            match target {
                ShellMembers::Empty => *target = ShellMembers::Faceuses(vec![faceuse]),
                ShellMembers::Faceuses(members) => members.push(faceuse),
                _ => return Err(invalid_members()),
            }
            brep.faceuse_mut(faceuse)?.shell = Some(recipient);
        }
        // Self-transfer clears the shell head but leaves this face-use cycle
        // connected. Preserve those native links independently of the head.
        refresh_faceuse_sibling_links(brep, recipient)?;
    }
    clear_shell_members(brep, source)
}

fn clear_shell_members(brep: &mut Brep, id: ShellId) -> Result<()> {
    let shell = brep.shell_mut(id)?;
    shell.shell_type = ShellType::Unknown;
    shell.members = ShellMembers::Empty;
    Ok(())
}

fn shell_list_size(brep: &Brep, id: ShellId) -> Result<usize> {
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
    kernel: &crate::kernel::Kernel,
    brep: &Brep,
    id: ShellId,
    tolerance: Option<f64>,
) -> Result<bool> {
    if brep.shell(id)?.is_vertex_shell() {
        return Ok(true);
    }
    let attempt = shell_bounds_for_degeneracy(kernel, brep, id)?;
    // Native IsDegenerate deliberately ignores CalculateBoundingBox's status.
    Ok(attempt
        .bounds
        .is_point_sized(tolerance.unwrap_or(crate::math::EFF_ZERO)))
}

/// Whether a region fits within `tolerance` (`SmRegion::IsDegenerate`).
/// `None` retains the native default `EFF_ZERO`.
///
/// # Errors
/// Invalid graph references or an unported bounding-box dependency. Ordinary
/// native box errors are ignored while retaining the initialized/partial box.
pub fn region_is_degenerate(
    kernel: &crate::kernel::Kernel,
    brep: &Brep,
    id: RegionId,
    tolerance: Option<f64>,
) -> Result<bool> {
    let attempt = region_bounds_for_degeneracy(kernel, brep, id)?;
    Ok(attempt
        .bounds
        .is_point_sized(tolerance.unwrap_or(crate::math::EFF_ZERO)))
}

// Outer failures mean a missing dependency or an unsafe native graph access;
// status is a real native return status, which the IsDegenerate callers ignore.
struct BoundsAttempt {
    bounds: crate::geom::extent3d::Extent3d,
    status: Result<()>,
}

// CO-PORTED minimum default-tight=false branches from query.face_p1. Keeping the
// status beside its box preserves partial writes and Native SER before Union.
fn region_bounds_for_degeneracy(
    kernel: &crate::kernel::Kernel,
    brep: &Brep,
    id: RegionId,
) -> Result<BoundsAttempt> {
    let mut bounds = crate::geom::extent3d::Extent3d::new();
    let shells = &brep.region(id)?.shells;
    if shells.is_empty() {
        return Ok(BoundsAttempt {
            bounds,
            status: Err(SolidError::sm_err("topology::shell::region_bounds")),
        });
    }
    for &shell in shells {
        let child = shell_bounds_for_degeneracy(kernel, brep, shell)?;
        if let Err(error) = child.status {
            return Ok(BoundsAttempt {
                bounds,
                status: Err(error),
            });
        }
        bounds = bounds.union(&child.bounds);
    }
    Ok(BoundsAttempt {
        bounds,
        status: Ok(()),
    })
}

fn shell_bounds_for_degeneracy(
    kernel: &crate::kernel::Kernel,
    brep: &Brep,
    id: ShellId,
) -> Result<BoundsAttempt> {
    let mut bounds = crate::geom::extent3d::Extent3d::new();
    let shell = brep.shell(id)?;
    match shell.shell_type.code() {
        16006 => {
            let vertex = shell.vertex(brep)?.ok_or(invalid_members())?;
            bounds.add_point(brep.vertex(vertex)?.point);
        }
        16005 => {
            for edge in wire_edges(brep, id)? {
                let child = edge_bounds_for_shell(kernel, brep, edge, false)?;
                if let Err(error) = child.status {
                    return Ok(BoundsAttempt {
                        bounds,
                        status: Err(error),
                    });
                }
                bounds = bounds.union(&child.bounds);
            }
        }
        16022 => {
            for faceuse in shell.get_faceuses()? {
                let face = brep.faceuse(faceuse)?.face.ok_or(invalid_members())?;
                let child = face_bounds_for_shell(kernel, brep, face, false)?;
                if let Err(error) = child.status {
                    return Ok(BoundsAttempt {
                        bounds,
                        status: Err(error),
                    });
                }
                bounds = bounds.union(&child.bounds);
            }
        }
        _ => {}
    }
    Ok(BoundsAttempt {
        bounds,
        status: Ok(()),
    })
}

fn edge_bounds_for_shell(
    kernel: &crate::kernel::Kernel,
    brep: &Brep,
    id: EdgeId,
    tight: bool,
) -> Result<BoundsAttempt> {
    use crate::geom::extent3d::Extent3d;
    use crate::nurbs::b_spline_curve::BoundingBoxRequest;
    brep.edge(id)?;
    let request = BoundingBoxRequest {
        normal: true,
        pseudo: false,
        polar: false,
    };
    match crate::query::edge::calculate_bounding_box(kernel, brep, id, request, tight) {
        Ok(boxes) => Ok(BoundsAttempt {
            bounds: boxes.normal.ok_or_else(|| {
                SolidError::invalid_input(
                    "topology::shell::edge_bounds_for_shell",
                    "requested 3D bounding box missing",
                )
            })?,
            status: Ok(()),
        }),
        // Native SER returns before unioning this edge's box. IsDegenerate
        // ignores that status and reads only the preceding successful aggregate.
        Err(error @ (SolidError::Upstream { .. } | SolidError::Nlib { .. })) => Ok(BoundsAttempt {
            bounds: Extent3d::new(),
            status: Err(error),
        }),
        // Missing dependencies and safety errors must not become ordinary
        // native status results or disappear into an initialized extent.
        Err(error) => Err(error),
    }
}

fn face_bounds_for_shell(
    _kernel: &crate::kernel::Kernel,
    brep: &Brep,
    id: crate::topology::FaceId,
    _tight: bool,
) -> Result<BoundsAttempt> {
    brep.face(id)?;
    // UNPORTED(query.face_p1): SmFace::CalculateBoundingBox
    Err(SolidError::unported(
        "SmFace::CalculateBoundingBox",
        "query.face_p1",
    ))
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
    /// Default B-rep bounding box when no radius was supplied.
    fn shell_ray_bounds(
        &self,
        _kernel: &crate::kernel::Kernel,
        _brep: crate::topology::BrepId,
    ) -> Result<crate::geom::extent3d::Extent3d> {
        // UNPORTED(query.brep): SmBrep::CalculateBoundingBox
        Err(SolidError::unported(
            "SmBrep::CalculateBoundingBox",
            "query.brep",
        ))
    }

    /// Classify the incoming line segment at its known intersection.
    fn shell_ray_sector(&self, _request: ShellRaySector<'_, O>) -> ShellRaySectorAttempt {
        // UNPORTED(topology.brep_p2): SmBrep::LocalCurveSector
        ShellRaySectorAttempt {
            status: Err(SolidError::unported(
                "SmBrep::LocalCurveSector",
                "topology.brep_p2",
            )),
            region_update: None,
        }
    }
}

/// Fire a sized ray at this shell's distinct boundary faces (`SmShell::RayFire`).
/// Native partial output writes survive an error in the returned attempt.
/// Missing geometry queries are explicit dependency errors.
pub fn ray_fire<O, W>(
    kernel: &crate::kernel::Kernel,
    brep: &Brep,
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
    use crate::geom::object::Shape;
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
            world
                .shell_ray_bounds(kernel, brep_id)?
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
        let mut seen = BTreeSet::new();
        let mut faces = Vec::new();
        for id in brep.shell(shell_id)?.get_faceuses()? {
            let faceuse = brep.faceuse(id)?;
            let face = faceuse.face.ok_or(SolidError::null_pointer(OP))?;
            if !seen.insert(face) {
                continue;
            }
            let mate = brep.faceuse(faceuse.mate.ok_or(SolidError::null_pointer(OP))?)?;
            if mate.shell != Some(shell_id) {
                faces.push((brep_id, face));
            }
        }
        let shape = Shape::new(&[], &faces, &[], &[])?;
        let vectors = [direction];
        let solutions = TopologySolver::shape_point_solve(
            kernel,
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
        let sector = world.shell_ray_sector(ShellRaySector {
            brep: brep_id,
            curve: &curve,
            interval: curve.natural_interval(),
            orientation: OrientType::Opposite,
            classification: &state.classification,
        });
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
        order_region_shells(&mut brep, region).unwrap();
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
            brep.shell(source).unwrap().mated_shells(&brep).unwrap(),
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
}
