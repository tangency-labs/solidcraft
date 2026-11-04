//! A sketch saves its equation curves (#8352) in `equations`: for each, the fit point spline it
//! places, its formulas (x(t) and y(t), or y = f(x) without `x`), the range of the variable and
//! the tolerance its points were chosen for. An older design has none, and its sketches mean
//! what they meant. The last release refuses a design that has them (it asks for an update)
//! instead of opening the splines as plain fixed splines that no longer follow their formulas.
