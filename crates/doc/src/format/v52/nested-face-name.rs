//! A face name that holds the name of another face writes it in parentheses: a blend face names
//! the edge it rounds (`F5:blend(F2:side:l2|F2:top)`) and an inner face names the face it offsets
//! (`F3:inner(F2:top)`), where before the name followed a colon (`F5:blend:F2:side:l2|F2:top`).
//! A curve id may hold a character outside ASCII (`F2:side:曲`), and the name that holds it
//! (`F5:blend(F2:side:曲|F2:top)`) is read a character at a time, so the character is read whole
//! and the name is not cut in half.
//! Older designs need no transformation: their names are in the older form and are read as the
//! faces they name, so a design opens as it did; the last release refuses a design saved with this
//! (the newer format number).
