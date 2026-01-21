use crate::application::session_key_spi_port::{SessionKey, SessionKeySpiPort};
use moka::sync::Cache;
use std::time::Duration;
use tracing::{debug, info};

pub struct SessionKeyMemoryCache {
    cache: Cache<String, SessionKey>,
}

impl Default for SessionKeyMemoryCache {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionKeyMemoryCache {
    pub fn new() -> SessionKeyMemoryCache {
        let cache = Cache::builder()
            .time_to_live(Duration::from_secs(600)) // TODO config
            .max_capacity(10_000) // TODO
            .build();

        SessionKeyMemoryCache { cache }
    }
}

impl SessionKeySpiPort for SessionKeyMemoryCache {
    fn store(
        &self,
        pake_session_id: &str,
        session_key: SessionKey,
    ) -> Result<(), crate::application::session_key_spi_port::ClientRepositoryError> {
        info!(
            "storing session key session_id: {} {:02X?}",
            pake_session_id, session_key
        );
        self.cache.insert(pake_session_id.to_string(), session_key);
        Ok(())
    }

    fn get(&self, pake_session_id: &str) -> Option<SessionKey> {
        match self.cache.get(pake_session_id) {
            Some(session_key) => {
                // Only log the session_key at debug level to avoid leaking sensitive info in production logs
                debug!(
                    "Found session key for {} -> {:?}",
                    pake_session_id, session_key
                );

                Some(session_key)
            }
            None => {
                // Note: This is not an error because a pake_session_id is returned in Authenticate start,
                //       but the session key is only stored after Authenticate finish.
                debug!("session key not found for session_id: {}", pake_session_id);
                // TODO: Remove this debug logging when we're done with initial development
                {
                    debug!("Cache entries count: {}", self.cache.entry_count());
                    for (key, _value) in self.cache.iter() {
                        debug!("Cache contains session_id: {}", key);
                    }
                }
                None
            }
        }
    }

    fn end_session(
        &self,
        pake_session_id: &str,
    ) -> Result<(), crate::application::session_key_spi_port::ClientRepositoryError> {
        self.cache.invalidate(pake_session_id);
        Ok(())
    }
}
