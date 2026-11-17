//! Saves explicit manufacturing grain directions in body construction coordinates, including
//! an explicit absence of grain. Older designs keep frame-member grain along their member
//! axes and leave other bodies without grain. The last release warns about the newer format,
//! opens what it understands and preserves the fields it cannot read on the next save.
