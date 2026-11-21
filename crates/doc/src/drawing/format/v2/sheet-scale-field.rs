//! A title block's cell or a note can read its sheet's scale (`{"source": "sheet", "key": "scale"}`,
//! #13059): the scale the sheet's base views share, none when they differ. An older drawing reads no such field.
//! The last release refuses a drawing saved with one, as a newer format.
