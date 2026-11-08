//! A table can order its rows by up to 4 of its columns (`sort`: `[{column, descending}]`, #13056),
//! each level of a parts list on its own, saved only when it is sorted. An older drawing's tables keep
//! their list's order. The last release refuses a drawing with this format's number.
