use crate::application::session_key_spi_port::{SessionKey, SessionKeySpiPort};
use moka::sync::Cache;
use std::time::{Duration, Instant};
use tracing::debug;

const SESSION_KEY_TTL_SECS: u64 = 600;

pub struct SessionKeyMemoryCache {
    cache: Cache<String, SessionKeyWithTimestamp>,
}

#[derive(Debug, Clone)]
pub struct SessionKeyWithTimestamp {
    pub session_key: SessionKey,
    pub inserted_at: Instant,
}

impl Default for SessionKeyMemoryCache {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionKeyMemoryCache {
    pub fn new() -> SessionKeyMemoryCache {
        let cache = Cache::builder()
            .time_to_live(Duration::from_secs(SESSION_KEY_TTL_SECS)) // TODO config
            .max_capacity(10_000) // TODO
            .build();

        SessionKeyMemoryCache { cache }
    }
}

impl SessionKeySpiPort for SessionKeyMemoryCache {
    fn store(
        &self,
        pake_session_id: &crate::domain::SessionId,
        session_key: SessionKey,
    ) -> Result<Duration, crate::application::session_key_spi_port::ClientRepositoryError> {
        debug!(
            "storing session key session_id: {} {:02X?}",
            pake_session_id.as_str(),
            session_key
        );

        let inserted_at = Instant::now();

        self.cache.insert(
            pake_session_id.as_str().to_string(),
            SessionKeyWithTimestamp {
                session_key,
                inserted_at,
            },
        );

        Ok(Duration::from_secs(SESSION_KEY_TTL_SECS))
    }

    fn get(&self, pake_session_id: &crate::domain::SessionId) -> Option<SessionKey> {
        match self.cache.get(pake_session_id.as_str()) {
            Some(session_key) => {
                // Only log the session_key at debug level to avoid leaking sensitive info in production logs
                debug!(
                    "Found session key for {} -> {:?}",
                    pake_session_id.as_str(),
                    session_key
                );

                Some(session_key.session_key)
            }
            None => {
                // Note: This is not an error because a pake_session_id is returned in Authenticate start,
                //       but the session key is only stored after Authenticate finish.
                debug!(
                    "session key not found for session_id: {}",
                    pake_session_id.as_str()
                );
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

    fn get_remaining_ttl(&self, pake_session_id: &crate::domain::SessionId) -> Option<Duration> {
        const TTL: Duration = Duration::from_secs(SESSION_KEY_TTL_SECS);
        self.cache.get(pake_session_id.as_str()).and_then(|entry| {
            let elapsed = entry.inserted_at.elapsed();
            TTL.checked_sub(elapsed)
        })
    }

    fn end_session(
        &self,
        pake_session_id: &crate::domain::SessionId,
    ) -> Result<(), crate::application::session_key_spi_port::ClientRepositoryError> {
        self.cache.invalidate(pake_session_id.as_str());
        Ok(())
    }
}
