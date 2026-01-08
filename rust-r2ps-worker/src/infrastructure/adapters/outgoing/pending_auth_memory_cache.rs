use crate::application::pending_auth_spi_port::{LoginSession, PendingAuthSpiPort};
use moka::sync::Cache;
use std::sync::Arc;
use std::time::Duration;

pub struct PendingAuthMemoryCache {
    start_auth: Cache<String, Arc<LoginSession>>,
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
    fn store_pending_auth(&self, client_id: &str, server_login_start_result: &Arc<LoginSession>) {
        self.start_auth
            .insert(client_id.to_string(), server_login_start_result.clone());
    }

    fn get_pending_auth(&self, client_id: &str) -> Option<Arc<LoginSession>> {
        match self.start_auth.remove(client_id) {
            Some(session) => Some(session),
            None => None,
        }
    }
}
