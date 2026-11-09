//! The renderer's box for a model's box: the camera's fit and depth range and the ground's ambient
//! occlusion take the renderer's own [`Bounds`] (the viewport knows no geometry crate), which a
//! model's [`Aabb3`] becomes here, at the boundary between them.

use solidcraft_geom::Aabb3;

use solidcraft_viewport::vec::Bounds;

/// The renderer's box with the corners of `b`; an empty box stays empty.
pub fn view_bounds(b: &Aabb3) -> Bounds {
    Bounds { min: b.min, max: b.max }
}

#[cfg(test)]
mod tests {
    use solidcore_record::Vec3;
    use solidcraft_geom::Aabb3;

    #[test]
    fn a_model_box_keeps_its_corners_and_an_empty_one_stays_empty() {
        let b = Aabb3 { min: Vec3::new(-1.0, 2.0, 3.0), max: Vec3::new(4.0, 5.0, 6.0) };
        let v = super::view_bounds(&b);
        assert_eq!((v.min, v.max), (b.min, b.max));
        assert_eq!((v.center(), v.diagonal()), (b.center(), b.diagonal()));
        assert!(super::view_bounds(&Aabb3::EMPTY).is_empty());
    }
}
