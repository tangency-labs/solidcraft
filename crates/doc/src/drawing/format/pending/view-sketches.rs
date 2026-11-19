//! A drawing's sketch can belong to a view (`"view": id`, #5425), drawn in model millimetres in the
//! view's plane and placed on paper at the view's scale. A sheet's sketch is saved as before
//! (`"sheet": id`, paper millimetres), so an older drawing needs no rewrite. The last release
//! (0.45.0) warns about the newer format number and opens a drawing whose sketches are all on
//! sheets as it is. It refuses a drawing with a view sketch, because its sketch record has no
//! `view` field.
