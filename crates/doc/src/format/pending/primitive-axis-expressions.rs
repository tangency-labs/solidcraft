//! A cone or coil's `axis` and a hole's `direction` can hold unitless expressions (strings) as
//! well as numbers (#21932), matching cylinders (#18822), so their direction can follow
//! parameters. An axis or direction of plain numbers is saved as before, and older designs need
//! no transformation.
