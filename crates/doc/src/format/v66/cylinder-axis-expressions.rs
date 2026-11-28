//! A cylinder's `axis` can hold unitless expressions (strings) as well as numbers, so a cylinder
//! whose direction comes from a parameter turns as the parameter changes. An axis of plain
//! numbers is saved as before, and older designs need no transformation. The last release opens
//! a design with such an axis with a newer-format warning; it cannot read that cylinder, so it
//! keeps the feature as one it cannot evaluate and saves it back as it came.
