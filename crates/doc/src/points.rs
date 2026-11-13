//! The points a feature holds, by what they are: a point that picks a face, any other
//! position, a direction. Each is a `Vec3` (and is saved as one), but a different type, so a
//! direction can't be moved like a position, and a pick can't be mistaken for either.

use std::ops::{Deref, DerefMut};

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;

macro_rules! point_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Vec3);

        impl Deref for $name {
            type Target = Vec3;
            fn deref(&self) -> &Vec3 {
                &self.0
            }
        }

        impl DerefMut for $name {
            fn deref_mut(&mut self) -> &mut Vec3 {
                &mut self.0
            }
        }

        impl From<Vec3> for $name {
            fn from(v: Vec3) -> Self {
                $name(v)
            }
        }

        impl From<$name> for Vec3 {
            fn from(p: $name) -> Vec3 {
                p.0
            }
        }
    };
}

point_type!(
    /// A point that picks a face: it moves with the feature, and names the face it picks.
    FacePick
);
point_type!(
    /// A position (an edge pick, an axis origin, a fold line): it moves with the feature.
    Position
);
point_type!(
    /// A direction: it turns with the feature, and never moves.
    Dir
);

/// The plain values of a list of picks, positions, directions or names (for code that takes
/// `Vec3`s or `String`s).
pub fn plain<T, P: Clone + Into<T>>(ps: &[P]) -> Vec<T> {
    ps.iter().map(|p| p.clone().into()).collect()
}
