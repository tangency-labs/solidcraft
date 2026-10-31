//! Identifiers: names and indices, each its own type so one cannot stand for another. In JSON
//! each is the string or number it holds.
//!
//! Persistent names ([`FaceName`], [`EdgeName`], [`VertexName`]) are opaque: compare them, keep
//! them, never parse them. What a name says is given as data next to it ([`super::Face::features`],
//! [`super::Edge::faces`]).

use std::fmt;

use serde::{Deserialize, Serialize};

macro_rules! name_type {
    ($($(#[$meta:meta])* $name:ident;)*) => {$(
        $(#[$meta])*
        #[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                $name(s.to_string())
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                $name(s)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        // A name's order, equality and hash are its string's, so maps keyed by it look up by `&str`.
        impl std::borrow::Borrow<str> for $name {
            fn borrow(&self) -> &str {
                &self.0
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }
    )*};
}

macro_rules! index_type {
    ($($(#[$meta:meta])* $name:ident($int:ty);)*) => {$(
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub $int);

        impl $name {
            /// As an index into the list it numbers.
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    )*};
}

name_type! {
    /// A body's name, unique in a snapshot.
    BodyName;
    /// A face's persistent name (`F1:-z`, `F2:side:l4`): it stays with the face when parameters change.
    FaceName;
    /// An edge's persistent name.
    EdgeName;
    /// A corner's persistent name.
    VertexName;
    /// A physical material's name.
    MaterialName;
    /// A parameter's name.
    ParameterName;
}

index_type! {
    /// A timeline feature: `F<id>` in persistent names.
    FeatureId(u64);
    /// A face's place in its body's face order.
    FaceIndex(u32);
    /// An edge's place in its body's edge order.
    EdgeIndex(u32);
    /// A corner's place in its body's corner order.
    VertexIndex(u32);
    /// A vertex of a body's mesh: an index into [`super::Body::positions`].
    MeshVertex(u32);
}
