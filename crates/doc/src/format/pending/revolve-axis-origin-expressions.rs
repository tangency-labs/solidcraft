//! A revolve's explicit axis `{origin, dir}` can save its origin's coordinates as length
//! expressions (`["W / 2", 0, "groove_floor"]`) so the axis follows the parameters; a plain number
//! stays a number. An older design has numbers only and reads as before. The last release refuses
//! a design with this change and asks for an update, instead of opening it and reading an
//! expression as the origin.
