//! The vector measures the reader takes on the record's [`Vec3`], computed by the kernel's
//! ([`solidcore::eval::vec::VecExt`], [`solidcore::Vec3`]) on the converted value.

use solidcore::eval::vec::VecExt;
use solidcore_convert::{record_vec3, vec3};
use solidcore_record::Vec3;

/// Products, lengths and directions of a record [`Vec3`].
pub(crate) trait Vec3Geom: Sized {
    fn dot(self, o: Self) -> f64;
    fn cross(self, o: Self) -> Self;
    fn len(self) -> f64;
    fn len2(self) -> f64;
    fn dist(self, o: Self) -> f64;
    /// The unit vector, `None` when shorter than 1e-9 or not finite.
    fn normalized(self) -> Option<Self>;
}

impl Vec3Geom for Vec3 {
    fn dot(self, o: Vec3) -> f64 {
        vec3(self).dot(vec3(o))
    }
    fn cross(self, o: Vec3) -> Vec3 {
        record_vec3(vec3(self).cross(vec3(o)))
    }
    fn len(self) -> f64 {
        vec3(self).len()
    }
    fn len2(self) -> f64 {
        vec3(self).len2()
    }
    fn dist(self, o: Vec3) -> f64 {
        vec3(self).dist(vec3(o))
    }
    fn normalized(self) -> Option<Vec3> {
        vec3(self).normalized().map(record_vec3)
    }
}
