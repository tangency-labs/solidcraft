//! SMLib's flat B-rep arrays (`capi/include/smlib.h`, "Flat B-rep arrays"): a B-rep as plain
//! int64 and double arrays laid out like a member of USD's BrepArray schema, which solidcore's body
//! is converted to and from ([`crate::import::Arrays`]).

use crate::{Brep, Result};

// Array ids and kind values, as in smlib.h.
pub const REGION_SHELL_COUNT: i32 = 1;
pub const REGION_TYPE: i32 = 2;
pub const SHELL_FACEUSE_COUNT: i32 = 3;
pub const SHELL_WIRE_EDGE_COUNT: i32 = 4;
pub const SHELL_POINT_TYPE: i32 = 5;
pub const FACEUSE_FACE: i32 = 6;
pub const FACEUSE_ORIENTATION: i32 = 7;
pub const FACE_LOOP_COUNT: i32 = 8;
pub const FACE_SURFACE_TYPE: i32 = 9;
pub const FACE_TRIM_TYPE: i32 = 10;
pub const LOOP_EDGEUSE_COUNT: i32 = 11;
pub const LOOP_VERTEX: i32 = 12;
pub const EDGEUSE_EDGE: i32 = 13;
pub const EDGEUSE_ORIENTATION: i32 = 14;
pub const EDGEUSE_NEXT_RADIAL: i32 = 15;
pub const EDGEUSE_RADIAL_ENTRY: i32 = 16;
pub const EDGE_CURVE_TYPE: i32 = 17;
pub const EDGE_VERTICES: i32 = 18;
pub const WIRE_EDGE_CURVE_TYPE: i32 = 19;
pub const WIRE_EDGE_VERTICES: i32 = 20;
pub const VERTEX_POINT_TYPE: i32 = 21;
pub const EDGE_NURBS_VERTEX_COUNT: i32 = 22;
pub const EDGE_NURBS_ORDER: i32 = 23;
pub const WIRE_EDGE_NURBS_VERTEX_COUNT: i32 = 24;
pub const WIRE_EDGE_NURBS_ORDER: i32 = 25;
pub const EDGEUSE_NURBS_VERTEX_COUNT: i32 = 26;
pub const EDGEUSE_NURBS_ORDER: i32 = 27;
pub const FACE_NURBS_U_VERTEX_COUNT: i32 = 28;
pub const FACE_NURBS_V_VERTEX_COUNT: i32 = 29;
pub const FACE_NURBS_U_ORDER: i32 = 30;
pub const FACE_NURBS_V_ORDER: i32 = 31;
pub const SOURCE: i32 = 32;
pub const FACE_NURBS_FORM: i32 = 33;
pub const EDGE_NURBS_FORM: i32 = 34;
pub const WIRE_EDGE_NURBS_FORM: i32 = 35;
/// Per face whose kind carries a definition (a ball patch), in face order: how many reals of
/// `FACE_DEFINITION_REALS` are its definition (a ball patch: 4).
pub const FACE_DEFINITION_REAL_COUNT: i32 = 36;
pub const TOLERANCE: i32 = 101;
pub const EXTENT: i32 = 102;
pub const FACE_RANGE: i32 = 103;
pub const EDGE_RANGE: i32 = 104;
pub const WIRE_EDGE_RANGE: i32 = 105;
pub const SHELL_POINT_POSITION: i32 = 106;
pub const VERTEX_POSITION: i32 = 107;
pub const EDGE_NURBS_CVS: i32 = 108;
pub const EDGE_NURBS_KNOTS: i32 = 109;
pub const EDGE_NURBS_WEIGHTS: i32 = 110;
pub const EDGE_CIRCLE_CENTER: i32 = 111;
pub const EDGE_CIRCLE_AXIS: i32 = 112;
pub const EDGE_CIRCLE_REF_DIRECTION: i32 = 113;
pub const EDGE_CIRCLE_RADIUS: i32 = 114;
pub const EDGE_LINE_ORIGIN: i32 = 115;
pub const EDGE_LINE_DIRECTION: i32 = 116;
pub const EDGE_ELLIPSE_CENTER: i32 = 117;
pub const EDGE_ELLIPSE_AXIS: i32 = 118;
pub const EDGE_ELLIPSE_REF_DIRECTION: i32 = 119;
pub const EDGE_ELLIPSE_X_RADIUS: i32 = 120;
pub const EDGE_ELLIPSE_Y_RADIUS: i32 = 121;
pub const WIRE_EDGE_NURBS_CVS: i32 = 122;
pub const WIRE_EDGE_NURBS_KNOTS: i32 = 123;
pub const WIRE_EDGE_NURBS_WEIGHTS: i32 = 124;
pub const WIRE_EDGE_CIRCLE_CENTER: i32 = 125;
pub const WIRE_EDGE_CIRCLE_AXIS: i32 = 126;
pub const WIRE_EDGE_CIRCLE_REF_DIRECTION: i32 = 127;
pub const WIRE_EDGE_CIRCLE_RADIUS: i32 = 128;
pub const WIRE_EDGE_LINE_ORIGIN: i32 = 129;
pub const WIRE_EDGE_LINE_DIRECTION: i32 = 130;
pub const WIRE_EDGE_ELLIPSE_CENTER: i32 = 131;
pub const WIRE_EDGE_ELLIPSE_AXIS: i32 = 132;
pub const WIRE_EDGE_ELLIPSE_REF_DIRECTION: i32 = 133;
pub const WIRE_EDGE_ELLIPSE_X_RADIUS: i32 = 134;
pub const WIRE_EDGE_ELLIPSE_Y_RADIUS: i32 = 135;
pub const EDGEUSE_NURBS_CVS: i32 = 136;
pub const EDGEUSE_NURBS_KNOTS: i32 = 137;
pub const EDGEUSE_NURBS_WEIGHTS: i32 = 138;
pub const FACE_NURBS_CVS: i32 = 139;
pub const FACE_NURBS_U_KNOTS: i32 = 140;
pub const FACE_NURBS_V_KNOTS: i32 = 141;
pub const FACE_NURBS_WEIGHTS: i32 = 142;
pub const FACE_SPHERE_CENTER: i32 = 143;
pub const FACE_SPHERE_AXIS: i32 = 144;
pub const FACE_SPHERE_REF_DIRECTION: i32 = 145;
pub const FACE_SPHERE_RADIUS: i32 = 146;
pub const FACE_PLANE_ORIGIN: i32 = 147;
pub const FACE_PLANE_AXIS: i32 = 148;
pub const FACE_PLANE_REF_DIRECTION: i32 = 149;
pub const FACE_CYLINDER_ORIGIN: i32 = 150;
pub const FACE_CYLINDER_AXIS: i32 = 151;
pub const FACE_CYLINDER_REF_DIRECTION: i32 = 152;
pub const FACE_CYLINDER_RADIUS: i32 = 153;
pub const FACE_CONE_ORIGIN: i32 = 154;
pub const FACE_CONE_AXIS: i32 = 155;
pub const FACE_CONE_REF_DIRECTION: i32 = 156;
pub const FACE_CONE_RADIUS: i32 = 157;
pub const FACE_CONE_SEMI_ANGLE: i32 = 158;
pub const FACE_TORUS_ORIGIN: i32 = 159;
pub const FACE_TORUS_AXIS: i32 = 160;
pub const FACE_TORUS_REF_DIRECTION: i32 = 161;
pub const FACE_TORUS_MAJOR_RADIUS: i32 = 162;
pub const FACE_TORUS_MINOR_RADIUS: i32 = 163;
pub const EDGE_TOLERANCE: i32 = 164;
pub const VERTEX_TOLERANCE: i32 = 165;
/// The definitions counted in `FACE_DEFINITION_REAL_COUNT`, packed in face order: a ball patch's
/// sphere's centre x, y, z and radius.
pub const FACE_DEFINITION_REALS: i32 = 166;
/// The largest EDGE_TOLERANCE or VERTEX_TOLERANCE import takes (smlib.h: "at most 1").
pub const MAX_TOLERANCE: f64 = 1.0;
/// `SMLIB_FLAT_MAX_LEN`: no array is longer.
pub const MAX_LEN: usize = 1 << 27;
pub const SURFACE_NURBS: u8 = 0;
pub const SURFACE_PLANE: u8 = 1;
pub const SURFACE_CYLINDER: u8 = 2;
pub const SURFACE_CONE: u8 = 3;
pub const SURFACE_SPHERE: u8 = 4;
pub const SURFACE_TORUS: u8 = 5;
/// A rational patch on a sphere (SMLib's `SmBallPatch`): its NURBS block, range and trims as a
/// NURBS face's, and its sphere in `FACE_DEFINITION_REALS`.
pub const SURFACE_BALL_PATCH: u8 = 6;
pub const CURVE_NURBS: u8 = 0;
pub const CURVE_LINE: u8 = 1;
pub const CURVE_CIRCLE: u8 = 2;
pub const CURVE_ELLIPSE: u8 = 3;

impl Brep {
    /// Whether SMLib finds this body one closed, manifold solid.
    pub fn is_manifold_solid(&self) -> Result<bool> {
        crate::core::is_manifold_solid(self.core())
    }
}
