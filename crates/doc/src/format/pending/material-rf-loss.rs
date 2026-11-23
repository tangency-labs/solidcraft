//! A design material may save how much a Wi-Fi signal loses each time a path crosses it, for the
//! Wi-Fi coverage analysis (`inspect.rf_coverage`): `rf_loss_db: [2.4 GHz, 5 GHz, 6 GHz]` in dB, on
//! the material in `user_materials`. An older design has none and needs none: the analysis then
//! uses the built-in material's losses or guesses from the wall's thickness. The last release warns
//! about the newer format, opens the design and drops the field when it saves.
