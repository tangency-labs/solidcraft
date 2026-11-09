//! A sketch's intersection link can save a curve of another sketch as either side (`a` or `b`,
//! `{"type": "sketch_curve", sketch, curve}`), standing in for that curve's extrusion along its
//! sketch's normal (#11048). An older design has only bodies and faces there, and opens as before.
//! The last release refuses a design that has one (it asks for an update) instead of opening the
//! link as lost.
