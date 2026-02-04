use crate::application::pending_auth_spi_port::{LoginSession, PendingAuthSpiPort};
use crate::domain::SessionId;
use moka::sync::Cache;
use std::sync::Arc;
use std::time::Duration;

pub struct PendingAuthMemoryCache {
    start_auth: Cache<SessionId, Arc<LoginSession>>,
}

impl Default for PendingAuthMemoryCache {
    fn default() -> Self {
        Self::new()
    }
}

impl PendingAuthMemoryCache {
    pub fn new() -> PendingAuthMemoryCache {
        let start_auth = Cache::builder()
            .time_to_live(Duration::from_secs(600)) // TODO config
            .max_capacity(10_000) // TODO
            .build();

        Self { start_auth }
    }
}

impl PendingAuthSpiPort for PendingAuthMemoryCache {
    fn store_pending_auth(&self, id: &SessionId, server_login_start_result: &Arc<LoginSession>) {
        self.start_auth
            .insert(id.clone(), server_login_start_result.clone());
    }

    fn get_pending_auth(&self, id: &SessionId) -> Option<Arc<LoginSession>> {
        self.start_auth.remove(id)
    }
}
