//! An equation curve can save `variable_unit: "range"`: its variable is a length, as its range is,
//! instead of a plain number (#9333). An older design has none, and its variable stays a plain
//! number, as before; a curve with a plain variable saves nothing new. The last release refuses a
//! design with this change and asks for an update, instead of opening it and losing the key on the
//! next save.
