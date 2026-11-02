//! A point cloud keeps the other properties its file gives its points (#8816): named numbers per
//! point in `complex` (`columns`, each whole-numbered or not, and the `names` a whole-numbered one's
//! values carry), and it can be coloured by one of them (`color_by: "property"`, with
//! `color_property`, the ramp over `height_range`) and shown only where `filters` keep it (ranges,
//! or sets of named values). Older designs need no transformation: their clouds have no other
//! properties and no filters. The last release refuses a design saved with this (the newer format
//! number) instead of opening its clouds without their properties, colouring and filters.
