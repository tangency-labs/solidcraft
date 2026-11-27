//! A sheet metal Fold names the pieces it parts a panel's cap into, where the faces around them do
//! not tell them apart (a round base flange's, whose rims are all one circle's): each is the cap
//! qualified by the fold and its part there (`F2:top#[F30:fold:stay]`, the part Fold F30 left where
//! it was; `F2:top#[F30:fold:1]`, the first part it folded). Designs save these names in references
//! made on a piece (a sketch attached to it). Older designs numbered such pieces by where they lie
//! (`F2:top#1`, `F2:top#2`); each piece keeps that number as its former name, so a reference saved
//! then still finds the same piece, and every result is unchanged. The last release opens a design
//! that holds the new names with a newer-format warning (`crate::newer_file`) and keeps what it
//! can't find for the next save.
