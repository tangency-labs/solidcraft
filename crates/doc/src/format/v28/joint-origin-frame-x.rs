//! A face or circle joint origin given no `x` reference keeps the x its frame had when picked
//! (`x` as a direction, component coordinates) and lays it onto the face as it is now, so a part
//! on a face that tilts keeps its turn. Older designs need no transformation: an origin without
//! `x` works its frame out from the normal alone, as before. The last release refuses a design
//! saved with this (the newer format number).
