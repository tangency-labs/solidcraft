//! The Import feature of a linked design (Insert Design) saves `link_faces`: the names that
//! design gives the faces of its bodies (by body, then face), so the linked faces are named after
//! them and joints on them follow them through Update Links (#3330). An older design has none
//! until its links are next updated; its linked faces keep their numbered names (`face:<k>`),
//! which they still answer to afterwards. The last release refuses a design that has them (it
//! refuses the newer format number).
