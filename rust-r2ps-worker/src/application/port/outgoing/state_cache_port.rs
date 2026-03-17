use crate::domain::DeviceHsmState;

/// In-memory cache for device state (moka-backed).
/// Used for read-only operations to avoid DB lookups.
pub trait StateCache: Send + Sync {
    fn get(&self, device_id: &str) -> Option<DeviceHsmState>;
    fn put(&self, device_id: &str, state: DeviceHsmState);
}

/// On-disk tamper detection cache (redb-backed).
/// Acts as a monotonic version witness — only moves forward.
/// Detects DB rollback attacks by comparing versions.
pub trait TamperDetectionCache: Send + Sync {
    /// Returns the last known version for a device.
    fn get(&self, device_id: &str) -> Option<u64>;
    /// Update the tamper cache with a new version (must be monotonically increasing).
    fn put(&self, device_id: &str, version: u64);
    /// Get the last snapshot offset for a partition.
    fn get_snapshot_offset(&self, partition: i32) -> Option<i64>;
    /// Persist snapshot offset for a partition.
    fn put_snapshot_offset(&self, partition: i32, offset: i64);
}
