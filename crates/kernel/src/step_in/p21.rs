//! The Part 21 parser lives in `solidcraft-step`; this reads with the kernel's progress watch.

pub use solidcraft_step::p21::{Entity, Exchange, Param, Record};

struct Watched(std::sync::Arc<dyn crate::watch::Watch>);

impl solidcraft_step::p21::Progress for Watched {
    fn cancelled(&self) -> bool {
        self.0.cancelled()
    }
    fn progress(&self, stage: &'static str, done: usize, total: usize) {
        self.0.progress(stage, done, total);
    }
}

/// [`solidcraft_step::p21::parse`] under the current [`crate::watch`].
pub fn parse(text: &str) -> Result<Exchange, String> {
    let watch = crate::watch::current().map(Watched);
    solidcraft_step::p21::parse_with(text, watch.as_ref().map(|w| w as &dyn solidcraft_step::p21::Progress))
}
