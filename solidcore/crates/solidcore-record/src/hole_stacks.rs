//! Hole stacks as data: the options a hole-stack query takes ([`Options`]) and its answer
//! ([`Stacks`]): the bores that line up through two or more touching bodies ([`Stack`], its
//! [`Layer`]s) and the bores in none ([`Lone`]), each with its slot shape ([`Slot`]). Plain data:
//! the query is the kernel's, through solidcore-ops' `hole_stacks`.
use serde::{Deserialize, Serialize};

use crate::Vec3;

/// What shapes the stacks: the gaps and offsets that hold, and which bores count.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Options {
    /// The largest gap between the layers of one stack, in mm.
    pub touching: f64,
    /// The largest offset between the axes of one line of holes, in mm.
    pub axis_tolerance: f64,
    /// Whether obround slots count.
    pub slots: bool,
    /// Diameter ranges in mm, each `min ≤ max`: only holes and slot widths inside a range
    /// count. Empty takes every size.
    pub diameters: Vec<(f64, f64)>,
}

/// An obround slot's shape across the axis.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Slot {
    /// The overall length, end to end, in mm.
    pub length: f64,
    /// The unit direction of the length.
    pub along: Vec3,
}

/// One layer of a stack: the bores of one body that run together.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    /// The body, by its index into the records the query took.
    pub body: usize,
    /// The layer's narrowest diameter, in mm.
    pub d: f64,
    /// The extent from the stack's start, in mm.
    pub t: [f64; 2],
    /// The narrowest bore's slot shape.
    pub slot: Option<Slot>,
}

/// Bores that line up through two or more touching bodies.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stack {
    /// Unit, from the start into the stack.
    pub axis: Vec3,
    /// The point on the axis the first layer starts at.
    pub start: Vec3,
    /// The clamped thickness, in mm.
    pub grip: f64,
    /// The point on the axis the last layer ends at.
    pub end: Vec3,
    /// The stack's layers, from its start along `axis`.
    pub layers: Vec<Layer>,
}

/// A bore in no stack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lone {
    /// The body, by its index into the records the query took.
    pub body: usize,
    /// The bore's entry point.
    pub at: Vec3,
    /// Unit axis, from the entry point into the bore.
    pub axis: Vec3,
    /// The bore's diameter, in mm.
    pub d: f64,
    /// The wall's extent, in mm.
    pub depth: f64,
    /// The slot's shape.
    pub slot: Option<Slot>,
}

/// The stacks among bodies' records, and the bores in none.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Stacks {
    /// The stacks, ordered by their start points.
    pub stacks: Vec<Stack>,
    /// The bores in no stack.
    pub lone: Vec<Lone>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn hole_stacks_data_round_trips_through_serde() {
        let s = Stacks {
            stacks: Vec::new(),
            lone: vec![Lone {
                body: 1,
                at: Vec3::ZERO,
                axis: Vec3::Z,
                d: 6.0,
                depth: 3.0,
                slot: Some(Slot {
                    length: 12.0,
                    along: Vec3::X,
                }),
            }],
        };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Stacks>(&json).unwrap(), s);
    }
}
