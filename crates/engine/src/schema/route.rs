//! Contracts and result types for the commands in `cmd/route.rs`: `route.create`, `route.edit`,
//! `route.move_point`, `route.delete`, `route.get`, `route.list` and `route.fittings`.
//!
//! The handlers read their parameters from the raw JSON, so each key here reads as its handler
//! reads it. A route spec's nested points, segments, fittings and valves are the planner's own
//! types (`crate::cmd::route_plan`), whose serde forms are the saved forms. Whether the route can
//! be planned (a straight tee run, a reducer whose span is its length, a size in the table) stays
//! the handler's to check.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::joints::OccurrenceDof;
use super::types::Point3;
use crate::cmd::route_plan::{Elbow, Fitting, Kind, Schedule, Segment, Valve};

/// Build a pipe route: a butt-welded line from centreline points and a pipe spec.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct CreateRouteParams {
    /// The route's name: 1…64 letters, digits, `-` or `_`. Its pieces are named
    /// `<name>_<kind>_<n>`.
    pub name: String,
    /// The route's pipe size (NPS). Points and segments without their own size take it.
    #[serde(default)]
    pub nps: Option<f64>,
    /// The pipe schedule. Default: 40.
    #[serde(default)]
    pub schedule: Schedule,
    /// The elbow a corner takes. Default: long radius.
    #[serde(default)]
    pub elbow: Elbow,
    /// The centreline as a polyline, each point to the next a segment at the route's `nps`.
    #[serde(default)]
    pub points: Vec<[f64; 3]>,
    /// Straight segments, each with its own size. A branch meets a run at a tee.
    #[serde(default)]
    pub segments: Vec<Segment>,
    /// The reducers, valves, caps, welds and open ends the route names.
    #[serde(default)]
    pub fittings: Vec<Fitting>,
    /// The valve library: each valve's STEP file, its ports and spindle in its own frame.
    #[serde(default)]
    pub valves: BTreeMap<String, Valve>,
    /// The end a point move holds still. Default: the first point.
    #[serde(default)]
    pub start: Option<[f64; 3]>,
    /// The display colours of pipe, fittings and valves, each `#rrggbb`.
    #[serde(default)]
    pub colours: Option<BTreeMap<String, String>>,
    /// A rigid row-major 3 × 4 matrix the whole route is taken through before it is built.
    #[serde(default)]
    pub frame: Option<[f64; 12]>,
}

/// Change a pipe route's centreline or spec; it is built again from the changed spec.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct EditRouteParams {
    /// The route, by name.
    pub route: String,
    /// The route's name. It must stay the same; the sub-assembly's component is renamed with
    /// `component.rename`.
    #[serde(default)]
    pub name: Option<String>,
    /// The route's pipe size (NPS).
    #[serde(default)]
    pub nps: Option<f64>,
    /// The pipe schedule.
    #[serde(default)]
    pub schedule: Option<Schedule>,
    /// The elbow a corner takes.
    #[serde(default)]
    pub elbow: Option<Elbow>,
    /// The centreline as a polyline.
    #[serde(default)]
    pub points: Option<Vec<[f64; 3]>>,
    /// Straight segments, each with its own size.
    #[serde(default)]
    pub segments: Option<Vec<Segment>>,
    /// The reducers, valves, caps, welds and open ends the route names.
    #[serde(default)]
    pub fittings: Option<Vec<Fitting>>,
    /// The valve library.
    #[serde(default)]
    pub valves: Option<BTreeMap<String, Valve>>,
    /// The end a point move holds still.
    #[serde(default)]
    pub start: Option<[f64; 3]>,
    /// The display colours of pipe, fittings and valves.
    #[serde(default)]
    pub colours: Option<BTreeMap<String, String>>,
    /// A rigid row-major 3 × 4 matrix the changed route is taken through.
    #[serde(default)]
    pub frame: Option<[f64; 12]>,
}

/// Move one point of a pipe route and build it again.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MoveRoutePointParams {
    /// The route, by name.
    pub route: String,
    /// The route point to move: a corner, a branch, an end, a weld or a fitting port.
    pub at: Point3,
    /// The move. Give `delta` or `to`, one of them.
    #[serde(default)]
    pub delta: Option<Point3>,
    /// The point's new place. Give `delta` or `to`, one of them.
    #[serde(default)]
    pub to: Option<Point3>,
    /// Whether the points beyond the moved one, away from the route's start, move with it.
    /// Default: true.
    #[serde(default)]
    pub carry: Option<bool>,
}

/// Name one pipe route.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct RouteNameParams {
    /// The route, by name.
    pub route: String,
}

/// `route.list` and `route.fittings` take no parameters.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct RouteListParams {}

/// One piece of a route: a pipe, an elbow, a tee, a reducer, a cap or a valve.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RoutePiece {
    /// Its name, `<route>_<kind>_<n>`.
    pub name: String,
    /// Its kind.
    pub kind: Kind,
    /// The fitting's size key (`elbow90_2`, `tee_1x0_75`) or the valve's library key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lib: Option<String>,
    /// A fitting's placement, a rigid row-major 3 × 4 matrix.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matrix: Option<[f64; 12]>,
    /// The points where it is welded to its neighbours.
    pub ports: Vec<[f64; 3]>,
    /// Its pipe size (NPS).
    pub nps: f64,
    /// A reducing tee's or reducer's small size (NPS).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nps_small: Option<f64>,
    /// A pipe's start.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub a: Option<[f64; 3]>,
    /// A pipe's end.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b: Option<[f64; 3]>,
    /// A pipe's cut length, in mm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length: Option<f64>,
    /// A valve's STEP file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    /// How far a valve's body runs past its span, in mm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlap: Option<f64>,
    /// An elbow's corner.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corner: Option<[f64; 3]>,
    /// An elbow's bend angle, in degrees.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub angle: Option<f64>,
    /// The piece's occurrence, by id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occurrence: Option<u64>,
    /// The bodies the piece made.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bodies: Option<Vec<String>>,
}

/// One pipe's cut length.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RouteCut {
    /// The pipe piece's name.
    pub piece: String,
    /// Its pipe size (NPS).
    pub nps: f64,
    /// Its schedule.
    pub schedule: String,
    /// Its cut length, in mm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length: Option<f64>,
}

/// What `route.create` and `route.edit` return.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RouteBuildResult {
    /// The route's name.
    pub route: String,
    /// Its sub-assembly component, by id.
    pub component: u64,
    /// That component's occurrence, by id.
    pub occurrence: u64,
    /// Every piece, in build order.
    pub pieces: Vec<RoutePiece>,
    /// One row per pipe.
    pub cut_list: Vec<RouteCut>,
    /// How many joints hold the pieces together.
    pub welds: usize,
    /// Occurrences with freedom left.
    pub free_dof: Vec<OccurrenceDof>,
    /// Pieces touching nothing held.
    pub unjoined: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `route.move_point` returns: the rebuilt route and how many points moved.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RouteMoveResult {
    /// The route's name.
    pub route: String,
    /// Its sub-assembly component, by id.
    pub component: u64,
    /// That component's occurrence, by id.
    pub occurrence: u64,
    /// Every piece, in build order.
    pub pieces: Vec<RoutePiece>,
    /// One row per pipe.
    pub cut_list: Vec<RouteCut>,
    /// How many joints hold the pieces together.
    pub welds: usize,
    /// Occurrences with freedom left.
    pub free_dof: Vec<OccurrenceDof>,
    /// Pieces touching nothing held.
    pub unjoined: Vec<String>,
    /// How many route points moved.
    pub moved_points: usize,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `route.get` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RouteGetResult {
    /// The route's name.
    pub route: String,
    /// Its sub-assembly component, by id.
    pub component: u64,
    /// The spec as saved: name, sizes, centreline, fittings and valves.
    pub spec: Value,
    /// The route's nodes, in order.
    pub points: Vec<[f64; 3]>,
    /// Every piece, in build order.
    pub pieces: Vec<RoutePiece>,
    /// One row per pipe.
    pub cut_list: Vec<RouteCut>,
    /// Each piece's bodies, by piece name.
    pub bodies: BTreeMap<String, Vec<String>>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// One route in `route.list`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RouteSummary {
    /// The route's name.
    pub name: String,
    /// Its sub-assembly component, by id.
    pub component: u64,
    /// Its first segment's pipe size (NPS).
    pub nps: Option<f64>,
    /// Its schedule.
    pub schedule: String,
    /// The elbow it takes.
    pub elbow: String,
    /// How many pieces it has.
    pub pieces: usize,
    /// How many of them are pipes.
    pub pipes: usize,
    /// The pipes' cut lengths added up, in mm.
    pub pipe_length: f64,
}

/// What `route.list` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RouteListResult {
    /// Every route in the design.
    pub routes: Vec<RouteSummary>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `route.delete` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RouteDeleteResult {
    /// The route's name.
    pub route: String,
    /// The components removed, by id.
    pub deleted: Vec<u64>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// A pipe size's walls, by schedule.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PipeWalls {
    /// Schedule 10S wall, in mm.
    #[serde(rename = "10S")]
    pub s10s: f64,
    /// Schedule 40 wall, in mm.
    #[serde(rename = "40")]
    pub s40: f64,
    /// Schedule 80 wall, in mm.
    #[serde(rename = "80")]
    pub s80: f64,
    /// ASTM B280 ACR copper tube wall, in mm. Null: not a copper size.
    #[serde(rename = "ACR")]
    pub acr: Option<f64>,
    /// EN 10253-4 stainless tube wall, in mm. Null: not an EN size.
    #[serde(rename = "EN")]
    pub en: Option<f64>,
}

/// One pipe size in the fitting library.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PipeSize {
    /// The pipe size (NPS).
    pub nps: f64,
    /// Its outside diameter, in mm.
    pub od: f64,
    /// Its ASME B36.10/B36.19 outside diameter, in mm.
    pub od_b3610: Option<f64>,
    /// Its EN 10253-4 outside diameter, in mm. Null: no EN tube in this size.
    pub od_en: Option<f64>,
    /// Its walls.
    pub wall: PipeWalls,
    /// Long radius elbow centre to end, in mm.
    pub elbow_long_radius: Option<f64>,
    /// Short radius elbow centre to end, in mm.
    pub elbow_short_radius: Option<f64>,
    /// Straight tee centre to end, in mm.
    pub tee: Option<f64>,
    /// Cap length, in mm.
    pub cap: Option<f64>,
}

/// A reducing tee's outlet.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ReducingTee {
    /// The run's pipe size (NPS).
    pub nps: f64,
    /// The branch's pipe size (NPS).
    pub nps_branch: f64,
    /// Centre to the branch's end, in mm.
    pub m: f64,
}

/// A concentric reducer's size.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ReducerSize {
    /// The large end's pipe size (NPS).
    pub nps: f64,
    /// The small end's pipe size (NPS).
    pub nps_small: f64,
    /// End to end, in mm.
    pub h: f64,
}

/// What `route.fittings` returns: the size tables routes are built from.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RouteFittingsResult {
    /// Every pipe size.
    pub pipes: Vec<PipeSize>,
    /// The reducing tees.
    pub reducing_tees: Vec<ReducingTee>,
    /// The concentric reducers.
    pub reducers: Vec<ReducerSize>,
    /// The schedules' names.
    pub schedules: Vec<String>,
    /// The elbows' names.
    pub elbows: Vec<String>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "route_tests.rs"]
mod tests;
