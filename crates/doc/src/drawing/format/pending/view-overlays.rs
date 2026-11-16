//! A drawing's views save the other positions of the assembly each draws over its own, in phantom
//! lines (`settings.overlays`, #13048): a list of up to 8 positions, each as `settings.pose` saves
//! one, followed down its projected views until one sets its own (`[]` included). An older drawing
//! has none, so each view draws its own position alone. The last release warns about the newer
//! format, opens what it can read and keeps the rest.
