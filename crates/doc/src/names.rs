//! What a feature refers to: bodies and features by name, sketches by id. Each is saved as the
//! plain name or id it wraps, but has its own type, so a body name can't be passed where a
//! feature name or an expression is meant.

use std::ops::Deref;

use serde::{Deserialize, Serialize};

macro_rules! name_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl Deref for $name {
            type Target = str;
            fn deref(&self) -> &str {
                &self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl std::borrow::Borrow<str> for $name {
            fn borrow(&self) -> &str {
                &self.0
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                $name(s)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                $name(s.to_string())
            }
        }

        impl From<&String> for $name {
            fn from(s: &String) -> Self {
                $name(s.clone())
            }
        }

        impl From<$name> for String {
            fn from(n: $name) -> String {
                n.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }

        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }

        impl PartialEq<String> for $name {
            fn eq(&self, other: &String) -> bool {
                &self.0 == other
            }
        }

        impl PartialEq<$name> for String {
            fn eq(&self, other: &$name) -> bool {
                *self == other.0
            }
        }

        impl PartialEq<$name> for str {
            fn eq(&self, other: &$name) -> bool {
                self == other.0
            }
        }
    };
}

name_type!(
    /// A body, by name.
    BodyName
);
name_type!(
    /// A feature, by name.
    FeatureName
);

/// A feature, by its id. Pattern and mirror seeds and construction planes name features by id
/// (#1134); a reference that resolves to nothing keeps id 0, which no feature ever has.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FeatureId(pub u64);

impl FeatureId {
    /// A reference that names no feature: a name that could not be resolved when an older
    /// design was migrated (#1134). Evaluation reports it like today's dangling name.
    pub const DANGLING: FeatureId = FeatureId(0);
}

impl Deref for FeatureId {
    type Target = u64;
    fn deref(&self) -> &u64 {
        &self.0
    }
}

impl From<u64> for FeatureId {
    fn from(id: u64) -> Self {
        FeatureId(id)
    }
}

impl From<FeatureId> for u64 {
    fn from(id: FeatureId) -> u64 {
        id.0
    }
}

impl PartialEq<u64> for FeatureId {
    fn eq(&self, other: &u64) -> bool {
        self.0 == *other
    }
}

impl PartialEq<FeatureId> for u64 {
    fn eq(&self, other: &FeatureId) -> bool {
        *self == other.0
    }
}

impl std::fmt::Display for FeatureId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// A sketch, by its feature id.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SketchId(pub u64);

impl Deref for SketchId {
    type Target = u64;
    fn deref(&self) -> &u64 {
        &self.0
    }
}

impl From<u64> for SketchId {
    fn from(id: u64) -> Self {
        SketchId(id)
    }
}

impl From<SketchId> for u64 {
    fn from(id: SketchId) -> u64 {
        id.0
    }
}

impl PartialEq<u64> for SketchId {
    fn eq(&self, other: &u64) -> bool {
        self.0 == *other
    }
}

impl PartialEq<SketchId> for u64 {
    fn eq(&self, other: &SketchId) -> bool {
        *self == other.0
    }
}

impl std::fmt::Display for SketchId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// The name of a copy of `name` (a pasted body, feature or component): the name and the next
/// number in brackets that `taken` doesn't hold — `Cube` → `Cube (2)`, another copy of `Cube` →
/// `Cube (3)`. A name that already ends in brackets takes the number right after them: a copy of
/// `Cube (2)` → `Cube (2)(2)`.
pub fn copy_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let name = name.trim();
    let gap = if name.ends_with(')') { "" } else { " " };
    (2..100_002u64).map(|i| format!("{name}{gap}({i})")).find(|c| !taken(c)).unwrap_or_else(|| format!("{name}{gap}(2)"))
}

#[cfg(test)]
mod copy_name_tests {
    use super::copy_name;

    #[test]
    fn a_copy_takes_the_next_free_number() {
        let taken = |names: &'static [&'static str]| move |n: &str| names.contains(&n);
        assert_eq!(copy_name("Cube", taken(&["Cube"])), "Cube (2)");
        assert_eq!(copy_name("Cube", taken(&["Cube", "Cube (2)"])), "Cube (3)");
        assert_eq!(copy_name("Cube (2)", taken(&["Cube", "Cube (2)", "Cube (3)"])), "Cube (2)(2)");
        assert_eq!(copy_name("Cube (2)", taken(&["Cube (2)", "Cube (2)(2)"])), "Cube (2)(3)");
        assert_eq!(copy_name("Sketch1", taken(&["Sketch1"])), "Sketch1 (2)");
    }
}
