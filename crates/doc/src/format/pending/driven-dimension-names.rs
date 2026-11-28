//! A driven (reference) dimension can save a parameter name (`param` on the constraint, and a
//! model parameter of that name) that expressions can use (#5582): the name a person gave it
//! (`sketch.name_driven`). Its value is what the dimension measures, read-only. An older design's
//! driven dimensions have no name, and stay without one: nothing in them is rewritten. The last
//! release warns about the newer format and treats the name as an ordinary parameter, whose value
//! stays what it measured when the design was saved instead of following the sketch.
