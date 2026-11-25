//! A cut with two sides names the two caps of its tool by the end they close: the low one
//! `bottom`, the high one `top`. A design saved before this named them by their height
//! instead, which gives both one name when the two sides of the cut lie on the same side of
//! the sketch plane: the two faces the cut leaves cannot be told apart then, and a sketch on
//! either of them cannot keep its attachment. A design that refers to such a face by the name
//! it had is told that the face has to be picked again, as it is for any face that was
//! renamed. The last release refuses a design that has it (it asks for an update).
