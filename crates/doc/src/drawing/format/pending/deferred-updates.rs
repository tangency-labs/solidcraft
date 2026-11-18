//! A drawing can say when its views follow a changed model (`updates`: `deferred`, only on
//! `drawing.update`), saved only when set. An older drawing follows the model as it changes. The last
//! release refuses a drawing with this format's number.
