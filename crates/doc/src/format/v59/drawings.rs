//! A design holds its drawings (#3890): `drawings`, each a drawing record of its own family
//! (`solidcraft-drawing/N`, `crate::drawing`). A design container keeps each drawing in its own
//! entry, `drawings/<id>.json`, beside `document.json`; a plain JSON design keeps them inline.
//! Older designs need no transformation: they hold no drawings. The last release refuses a design
//! saved with this (the newer format number), so it never opens one and saves it without them.
