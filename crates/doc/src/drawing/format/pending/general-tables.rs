//! A table can hold cells typed in (`kind: {general: {rows: [{key: text}]}}`, #13056), each row its
//! cells by column key, as plain text. An older drawing has no general tables. The last release
//! refuses a drawing with this format's number.
