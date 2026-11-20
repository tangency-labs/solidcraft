//! Drawings save their views (`views`, #4816): base views of their designs and the views projected
//! from them, on their sheets, each with the settings it sets itself; and the projection every
//! projected view follows (`projection`: `first` or `third` angle). An older drawing has no views,
//! and its projection is third angle. The last release refuses a drawing saved with them, as a
//! newer format.
