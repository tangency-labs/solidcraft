//! How high a hole's entry face stands over its rim ([`solidcore::query::hole_mouth`]).

use solidcore::query::hole_mouth as hm;
use solidcore::Point3;

use crate::body::Body;
use crate::session::with_body;
use crate::{Error, Result};

impl Body {
    /// [`hm::hole_mouth_lip`]: how high the faces entered at `spot` along `dir` stand over the
    /// rim of `radius`; [`Error::Unsupported`] for a face that is neither planar nor convex.
    pub fn hole_mouth_lip(
        &self,
        spot: [f64; 3],
        dir: [f64; 3],
        radius: f64,
    ) -> Result<Option<f64>> {
        if !(spot.iter().chain(&dir).all(|x| x.is_finite()) && radius.is_finite()) {
            return Err(Error::Argument);
        }
        let (spot, dir) = (
            Point3::new(spot[0], spot[1], spot[2]),
            Point3::new(dir[0], dir[1], dir[2]),
        );
        with_body("hole_mouth_lip", self, |k, b| {
            hm::hole_mouth_lip(k, b, spot, dir, radius).map(Ok)
        })?
    }
}

#[cfg(test)]
mod tests {
    use crate::boolean::{boolean, BooleanOp};
    use crate::{make_box, make_cylinder, make_sphere, session, Error};

    fn all_fixes() {
        session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    }

    /// 30° off the normal, leaning 22.5° from +x: the face stands r tan 30° over the rim.
    #[test]
    fn hole_mouth_lip_of_a_box_face_entered_at_an_angle_is_the_rim_height() {
        all_fixes();
        let b = make_box([0.0, 0.0, 0.0], [40.0, 40.0, 20.0]).unwrap();
        // (sin 30° cos 22.5°, sin 30° sin 22.5°, -cos 30°)
        let dir = [
            0.461_939_766_255_643_4,
            0.191_341_716_182_544_9,
            -0.866_025_403_784_438_6,
        ];
        let radius = 3.0;
        let lip = b
            .hole_mouth_lip([20.0, 20.0, 20.0], dir, radius)
            .unwrap()
            .unwrap();
        let want = 3.0 * 0.577_350_269_189_625_8;
        assert!((lip - want).abs() < 1e-9 * want, "lip {lip} vs {want}");
        assert_eq!(
            b.hole_mouth_lip([20.0, 20.0, 20.0], [0.0, 0.0, -1.0], radius)
                .unwrap(),
            Some(0.0)
        );
        // In the air, inside the material, and on a face the axis leaves by: no entry face.
        assert_eq!(
            b.hole_mouth_lip([20.0, 20.0, 25.0], dir, radius).unwrap(),
            None
        );
        assert_eq!(
            b.hole_mouth_lip([20.0, 20.0, 10.0], dir, radius).unwrap(),
            None
        );
        assert_eq!(
            b.hole_mouth_lip([20.0, 20.0, 0.0], dir, radius).unwrap(),
            None
        );
    }

    /// An edge takes its higher face, a convex wall its tangent plane; a bore's wall is refused.
    #[test]
    fn hole_mouth_lip_on_an_edge_a_round_wall_and_a_bore() {
        all_fixes();
        let b = make_box([0.0, 0.0, 0.0], [40.0, 40.0, 20.0]).unwrap();
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let lip = b
            .hole_mouth_lip([40.0, 20.0, 20.0], [-s, 0.0, -s], 3.0)
            .unwrap()
            .unwrap();
        assert!((lip - 3.0).abs() < 1e-9, "{lip}");
        let bar = make_cylinder([0.0, 0.0, 0.0], 10.0, 40.0).unwrap();
        let radial = bar
            .hole_mouth_lip([10.0, 0.0, 20.0], [-1.0, 0.0, 0.0], 2.0)
            .unwrap()
            .unwrap();
        assert!(radial.abs() < 1e-9, "{radial}");
        let leaning = bar
            .hole_mouth_lip([10.0, 0.0, 20.0], [-s, 0.0, -s], 2.0)
            .unwrap()
            .unwrap();
        assert!((leaning - 2.0).abs() < 1e-9, "{leaning}");
        let ball = make_sphere([0.0, 0.0, 0.0], 5.0).unwrap();
        let top = ball
            .hole_mouth_lip([0.0, 5.0, 0.0], [0.0, -1.0, 0.0], 1.0)
            .unwrap()
            .unwrap();
        assert!(top.abs() < 1e-9, "{top}");
        let bore = make_cylinder([0.0, 0.0, -1.0], 6.0, 42.0).unwrap();
        let tube = boolean(bar, bore, BooleanOp::Subtract).unwrap().unwrap();
        assert!(matches!(
            tube.hole_mouth_lip([6.0, 0.0, 20.0], [1.0, 0.0, 0.0], 2.0),
            Err(Error::Unsupported(_))
        ));
        // The apex of a drill point's pit has no normal.
        let block = make_box([0.0, 0.0, 0.0], [40.0, 30.0, 20.0]).unwrap();
        let tip = crate::make_cone([20.0, 15.0, 0.0], 4.0, 0.0, 2.4).unwrap();
        let pit = boolean(block, tip, BooleanOp::Subtract).unwrap().unwrap();
        assert!(matches!(
            pit.hole_mouth_lip([20.0, 15.0, 2.4], [0.0, 0.0, 1.0], 6.0),
            Err(Error::Unsupported(_))
        ));
    }
}
