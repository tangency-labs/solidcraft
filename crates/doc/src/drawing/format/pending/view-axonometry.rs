//! A drawing's pictorial views can be dimetric or drawn at three scales of their own (`axonometry`,
//! #13044): `"dimetric"` (ISO 5456-3's, the depth at half) or `{"scales": [x, y, depth]}`, written in
//! the pictorial view's kind beside its parent and diagonal, and only when it isn't isometric. An
//! older drawing's pictorial views are isometric. The last release warns about the newer format
//! number, opens what it can read and keeps the rest.
