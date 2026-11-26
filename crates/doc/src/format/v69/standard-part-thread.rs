//! A standard part (`parts`, by component id) can save how its metric thread is made: `thread`,
//! `"modeled"` (cut into the body), so Change Part Size rebuilds it the same way. It is left
//! out for a cosmetic thread, and an older design has no `thread`: its parts keep the cosmetic
//! thread they were made with. The last release warns about the newer format number; the part's
//! features are ordinary ones it reads as they are, and it does not know the new key, so resizing
//! the part there rebuilds it with a cosmetic thread.
