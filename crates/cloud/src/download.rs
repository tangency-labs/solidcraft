//! Verified immutable download objects, persisted independently of complete design heads.
use crate::Result;
use std::future::Future;

pub trait DownloadCache {
    fn load(&self, path: &str) -> impl Future<Output = Result<Option<Vec<u8>>>>;
    fn store(&self, path: &str, bytes: &[u8]) -> impl Future<Output = Result<()>>;
}

#[cfg(feature = "github")]
pub(crate) struct Uncached;
#[cfg(feature = "github")]
impl DownloadCache for Uncached {
    async fn load(&self, _: &str) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
    async fn store(&self, _: &str, _: &[u8]) -> Result<()> {
        Ok(())
    }
}
