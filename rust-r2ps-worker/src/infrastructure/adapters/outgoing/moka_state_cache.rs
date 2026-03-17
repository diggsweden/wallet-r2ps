use crate::application::port::outgoing::state_cache_port::StateCache;
use crate::domain::DeviceHsmState;
use moka::sync::Cache;
use std::time::Duration;

/// In-memory state cache backed by moka.
/// Configurable max_capacity with 1-hour time-to-idle eviction.
pub struct MokaStateCache {
    cache: Cache<String, DeviceHsmState>,
}

impl MokaStateCache {
    pub fn new(max_capacity: u64, ttl_secs: u64) -> Self {
        let cache = Cache::builder()
            .max_capacity(max_capacity)
            .time_to_idle(Duration::from_secs(ttl_secs))
            .build();

        Self { cache }
    }
}

impl StateCache for MokaStateCache {
    fn get(&self, device_id: &str) -> Option<DeviceHsmState> {
        self.cache.get(&device_id.to_string())
    }

    fn put(&self, device_id: &str, state: DeviceHsmState) {
        self.cache.insert(device_id.to_string(), state);
    }
}
