//! A saved GPU path setup can keep an indirect-light clamp (#9815): `clamp_indirect` in its job
//! options, a linear radiance from 0.000001 to 1000000 that caps each contribution from the
//! first bounce on. It is omitted when off. An older design has none, and its setups render
//! as before. The last release refuses a design saved with it and asks for an update, instead
//! of failing to read the setup or rendering it without the clamp.
