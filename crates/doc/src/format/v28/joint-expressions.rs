//! A joint keeps the expressions its offset, angle and values were typed as (`exprs`) and works
//! them out again whenever the design changes, so it follows the parameters they name. Older
//! designs need no transformation: a joint without `exprs` keeps its numbers as before. The last
//! release refuses a design saved with this (the newer format number).
