use crate::application::session_key_spi_port::{SessionKey, SessionKeySpiPort};
use crate::domain::SessionId;
use moka::sync::Cache;
use std::time::{Duration, Instant};
use tracing::debug;

const SESSION_KEY_TTL_SECS: u64 = 600;

pub struct SessionKeyMemoryCache {
    cache: Cache<SessionId, SessionKeyWithTimestamp>,
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
        id: &SessionId,
        session_key: SessionKey,
    ) -> Result<Duration, crate::application::session_key_spi_port::ClientRepositoryError> {
        debug!("Storing session key session_id: {:?} {:?}", id, session_key);

        let inserted_at = Instant::now();

        self.cache.insert(
            id.clone(),
            SessionKeyWithTimestamp {
                session_key,
                inserted_at,
            },
        );

        Ok(Duration::from_secs(SESSION_KEY_TTL_SECS))
    }

    fn get(&self, id: &SessionId) -> Option<SessionKey> {
        match self.cache.get(id) {
            Some(session_key) => {
                // Only log the session_key at debug level to avoid leaking sensitive info in production logs
                debug!("Found session key for {:?} -> {:?}", id, session_key);

                Some(session_key.session_key)
            }
            None => {
                // Note: This is not an error because a pake_session_id is returned in Authenticate start,
                //       but the session key is only stored after Authenticate finish.
                debug!("session key not found for session_id: {:?}", id);
                // TODO: Remove this debug logging when we're done with initial development
                {
                    debug!("Cache entries count: {}", self.cache.entry_count());
                    for (key, _value) in self.cache.iter() {
                        debug!("Cache contains session_id: {:?}", key);
                    }
                }
                None
            }
        }
    }

    fn get_remaining_ttl(&self, id: &SessionId) -> Option<Duration> {
        const TTL: Duration = Duration::from_secs(SESSION_KEY_TTL_SECS);
        self.cache.get(id).and_then(|entry| {
            let elapsed = entry.inserted_at.elapsed();
            TTL.checked_sub(elapsed)
        })
    }

    fn end_session(
        &self,
        id: &crate::domain::SessionId,
    ) -> Result<(), crate::application::session_key_spi_port::ClientRepositoryError> {
        self.cache.invalidate(id);
        Ok(())
    }
}
