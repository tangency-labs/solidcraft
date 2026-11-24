//! Title block cells for ISO 7200 fields (#20121). A title block's cell or a note can read its
//! drawing's language (`{"source": "drawing", "key": "language"}`), the drawing's language code in
//! capitals, and a drawing border's profile can say whether its frame prints the zone letters and
//! numbers and the sheet's size (`"zone_labels": true`). An older drawing has neither: it keeps
//! the title block it was saved with, and its border prints no labels, as it never did.
//! The last release refuses a drawing saved with either, as a newer format.
