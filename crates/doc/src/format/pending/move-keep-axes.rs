//! A Move saves `carry` and `leave`, the construction features (by feature id) it takes along
//! with its bodies or leaves where they are, when either list is not empty: Keep Axes With Body
//! carries a Center Pole held by fixed points. An older design has neither and needs neither: its
//! Moves carry only the constructions built on the moved bodies, as before. The last release
//! warns about the newer format number, opens the design and keeps both fields when it saves it;
//! there a carried pole stays where it was drawn and a left one follows its body.
