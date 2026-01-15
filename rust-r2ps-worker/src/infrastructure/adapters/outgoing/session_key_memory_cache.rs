use crate::application::session_key_spi_port::{SessionKey, SessionKeySpiPort};
use moka::sync::Cache;
use std::time::Duration;
use tracing::info;

pub struct SessionKeyMemoryCache {
    cache: Cache<String, SessionKey>,
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
        info!("get session key session_id: {}", pake_session_id);

        match self.cache.get(pake_session_id) {
            Some(session_key) => {
                info!(
                    "get session key session_id: {} {:02X?}",
                    pake_session_id, session_key
                );

                Some(session_key)
            }
            None => None,
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
