//! Turning SolidCraft's planar regions into the engine's inputs: curve-true
//! geometries for the exact profile passes, flattened polygons for the
//! clearing planners. Splines and other non-line-and-arc curves are
//! followed as line chains within the operation's tolerance, as everywhere
//! else in the crate.

use raygeo::geo::types::{Point, Polygon};
use solidcraft_geom::{Loop2, Region2, Vec2};

/// Appends one closed loop to `g` at Z 0 (the engine lifts every move to
/// the pass height itself). Lines and arcs stay exact; other curves become
/// line chains within `tolerance`.
fn append_loop(g: &mut raygeo::geo::geometry::Geometry, lp: &Loop2, tolerance: f64) {
    let mut current = Vec2::ZERO;
    let mut first = true;
    for seg in &lp.segs {
        match *seg {
            solidcraft_geom::Seg2::Line { a, b } => {
                if first {
                    g.move_to(a.x, a.y, 0.0);
                    first = false;
                }
                g.line_to(b.x, b.y, 0.0);
                current = b;
            }
            solidcraft_geom::Seg2::Arc { center, radius, start, sweep } => {
                let a = center + Vec2::from_angle(start) * radius;
                let b = center + Vec2::from_angle(start + sweep) * radius;
                if first {
                    g.move_to(a.x, a.y, 0.0);
                    first = false;
                } else if a.dist(current) > 1e-9 {
                    g.line_to(a.x, a.y, 0.0);
                }
                g.arc_to(b.x, b.y, center.x - a.x, center.y - a.y, sweep < 0.0, 0.0);
                current = b;
            }
            ref other => {
                let pts = other.polyline(tolerance.max(1e-4));
                for p in pts {
                    if first {
                        g.move_to(p.x, p.y, 0.0);
                        first = false;
                    } else if p.dist(current) > 1e-9 {
                        g.line_to(p.x, p.y, 0.0);
                    }
                    current = p;
                }
            }
        }
    }
    g.close_path();
}

/// A region as one curve-true geometry: the outer loop and its holes as
/// separate closed subpaths. The engine reads the nesting for islands
/// itself.
pub(crate) fn loop_group(region: &Region2, tolerance: f64) -> raygeo::geo::geometry::Geometry {
    let mut g = raygeo::geo::geometry::Geometry::new();
    append_loop(&mut g, &region.outer, tolerance);
    for hole in &region.holes {
        append_loop(&mut g, hole, tolerance);
    }
    g
}

/// A single loop as a curve-true geometry.
pub(crate) fn loop_geometry(lp: &Loop2, tolerance: f64) -> raygeo::geo::geometry::Geometry {
    let mut g = raygeo::geo::geometry::Geometry::new();
    append_loop(&mut g, lp, tolerance);
    g
}

/// A loop as a flattened polygon for the clearing planners, wound
/// counter-clockwise whatever the region's own convention is.
pub(crate) fn loop_polygon(lp: &Loop2, tolerance: f64) -> Polygon {
    lp.ccw().polyline(tolerance.max(1e-4)).into_iter().map(|p| Point::new(p.x, p.y)).collect()
}

/// The boundary polygon and island polygons of one region, for the clearing
/// planners.
pub(crate) fn region_polygons(region: &Region2, tolerance: f64) -> (Polygon, Vec<Polygon>) {
    let boundary = loop_polygon(&region.outer, tolerance);
    let islands = region.holes.iter().map(|h| loop_polygon(h, tolerance)).collect();
    (boundary, islands)
}
