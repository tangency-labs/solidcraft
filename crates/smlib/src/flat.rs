//! SMLib's flat B-rep arrays (`capi/include/smlib.h`, "Flat B-rep arrays"): a B-rep as plain
//! int64 and double arrays laid out like a member of USD's BrepArray schema, read from SMLib with
//! [`Brep::export`](crate::Brep::export) and built into SMLib with [`Flat::import`].

use crate::{Brep, Result, error, ffi, init};

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
pub const CURVE_NURBS: u8 = 0;
pub const CURVE_LINE: u8 = 1;
pub const CURVE_CIRCLE: u8 = 2;
pub const CURVE_ELLIPSE: u8 = 3;

/// One B-rep's flat arrays, owned on the C side.
pub struct Flat(ffi::flat::FlatHandle);

impl std::fmt::Debug for Flat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("smlib::Flat")
    }
}

impl Flat {
    /// An empty set of arrays.
    pub fn new() -> Result<Flat> {
        init()?;
        ffi::flat::new().map(Flat).map_err(error)
    }

    /// The arrays of `a` (see [`crate::import::arrays`]).
    pub fn from_arrays(a: &crate::import::Arrays) -> Result<Flat> {
        let mut f = Flat::new()?;
        for (id, v) in &a.ints {
            f.set_i64(*id, v)?;
        }
        for (id, v) in &a.reals {
            f.set_f64(*id, v)?;
        }
        Ok(f)
    }

    pub fn get_i64(&self, id: i32) -> Result<Vec<i64>> {
        ffi::flat::get_i64(&self.0, id).map_err(error)
    }
    pub fn get_f64(&self, id: i32) -> Result<Vec<f64>> {
        ffi::flat::get_f64(&self.0, id).map_err(error)
    }
    pub fn set_i64(&mut self, id: i32, v: &[i64]) -> Result<()> {
        crate::check(ffi::flat::set_i64(&mut self.0, id, v))
    }
    pub fn set_f64(&mut self, id: i32, v: &[f64]) -> Result<()> {
        crate::check(ffi::flat::set_f64(&mut self.0, id, v))
    }

    /// The SMLib B-rep these arrays describe (checked on the C side before SMLib sees them).
    pub fn import(&self) -> Result<Brep> {
        init()?;
        ffi::flat::import(&self.0).map(Brep::made).map_err(error)
    }
}

impl Brep {
    /// This body as flat arrays.
    pub fn export(&self) -> Result<Flat> {
        ffi::flat::export(self.cpp()?).map(Flat).map_err(error)
    }

    /// Whether SMLib finds this body one closed, manifold solid.
    pub fn is_manifold_solid(&self) -> Result<bool> {
        if crate::route::Op::Measure.backend() == crate::route::Backend::Solidcore {
            return crate::core::is_manifold_solid(self.core()?);
        }
        ffi::flat::is_manifold_solid(self.cpp()?).map_err(error)
    }
}
