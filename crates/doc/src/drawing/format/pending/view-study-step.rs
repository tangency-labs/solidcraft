//! A drawing view's position may name a step of one of the design's motion studies (`settings.pose`,
//! #13048): `{"study": {"id": <the study's id>, "step": <a whole step>}}`, beside `"designed"` and
//! `{"joints": [...]}`, followed down its projected views until one sets its own. An older drawing has
//! none, so its views draw as before. The last release refuses a drawing saved with one, as a newer
//! format.
