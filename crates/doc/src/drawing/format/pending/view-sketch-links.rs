//! A view's sketch may link to the model edges its view draws (`links`, each a `view_edge` source
//! naming the edge as an edge reference does, #5425), with the curves each link last made. An
//! older drawing's sketches have no links. The last release warns about the newer format number,
//! opens the rest of the drawing and keeps the sketches as they were.
