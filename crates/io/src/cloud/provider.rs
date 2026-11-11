//! Provider API operations share native recovery and receipts with standard Git repositories.
use solidcraft_cloud::{Connection, Destination, PendingSave, Published, Result, Snapshot, StoredDesign, management::History};

/// Blocking, worker-local boundary. Hosting adapters own HTTP/authentication, never local queues.
/// Adding a host implements this interface and registers it through `NativeAccess`.
pub trait ProviderRepository {
    fn test(&mut self) -> Result<()>;
    fn list(&mut self, stored: &mut [StoredDesign]) -> Result<Vec<Destination>>;
    fn list_cached(&mut self, stored: &mut [StoredDesign], _cache: &super::LocalOutbox) -> Result<Vec<Destination>> {
        self.list(stored)
    }
    fn trash(&mut self) -> Result<Vec<Destination>>;
    fn open(&mut self, destination: &Destination, cache: &super::LocalOutbox) -> Result<Snapshot>;
    /// Check repository metadata before reusing a locally confirmed snapshot.
    fn is_current(&mut self, _cached: &StoredDesign, _cache: &super::LocalOutbox) -> Result<bool> {
        Ok(false)
    }
    fn thumbnail(&mut self, destination: &Destination) -> Result<Option<Vec<u8>>>;
    fn history(&mut self, destination: &Destination) -> Result<History>;
    fn open_at(&mut self, destination: &Destination, commit: &str) -> Result<Snapshot>;
    fn publish(&mut self, save: &PendingSave, snapshot: Snapshot, parent: Option<&Published>) -> Result<Published>;
}

mod worker;
pub(super) use worker::{refresh, run};

pub(super) fn connection(operation: &solidcraft_cloud::jobs::Operation) -> Option<&Connection> {
    use solidcraft_cloud::jobs::Operation;
    match operation {
        Operation::Setup(c)
        | Operation::Test(c)
        | Operation::List(c)
        | Operation::Synchronize(c)
        | Operation::Trash(c)
        | Operation::Save { connection: c, .. }
        | Operation::Open { connection: c, .. }
        | Operation::Thumbnail { connection: c, .. }
        | Operation::History { connection: c, .. }
        | Operation::Manage { connection: c, .. }
        | Operation::Discard { connection: c, .. } => Some(c),
        _ => None,
    }
}
