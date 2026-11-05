//! A design saves the print face of its bodies (`print_faces`, by body name): the flat face a
//! slicer export lays on the print bed, held by the same face target a sketch on a face holds.
//! An older design has none, and every body is exported as it lies, as before. The last
//! release refuses a design that has print faces and asks for an update, instead of opening it
//! and losing them on the next save.
