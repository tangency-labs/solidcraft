//! A sheet metal rule may save the material it is for (`"material": "Aluminium 5052-H32"`), as
//! the standard library or a rule pack names it or as typed in the Sheet Metal Rules manager
//! (#18304). It is a label: no value of the rule or the sheet is computed from it. It is saved only
//! when set, so a design with no material on its rules saves as before, and older designs open
//! with none and build as they did. The last release opens a design that has it with a warning
//! about the newer format number, and keeps the field it doesn't read.
