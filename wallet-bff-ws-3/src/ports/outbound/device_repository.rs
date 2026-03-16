use crate::domain::device_management::{
    entities::Device,
    errors::DeviceError,
    value_objects::{ClientId, DeviceState},
};

/// Port for persisting and retrieving device state.
///
/// Implementations handle the storage mechanism (Redis, in-memory, etc.)
/// while the application layer depends only on this trait.
pub trait DeviceRepository: Send + Sync {
    /// Store a device with its current state.
    fn save(&self, device: &Device) -> impl Future<Output = Result<(), DeviceError>> + Send;

    /// Find a device by its identifier.
    fn find_by_id(
        &self,
        id: &ClientId,
    ) -> impl Future<Output = Result<Option<Device>, DeviceError>> + Send;

    /// Check whether a device exists.
    fn exists(&self, id: &ClientId) -> impl Future<Output = Result<bool, DeviceError>> + Send;

    /// Store raw device state for a given client ID.
    /// Used during device initialization when we don't yet have a full Device aggregate.
    fn store_state(
        &self,
        id: &ClientId,
        state: &DeviceState,
    ) -> impl Future<Output = Result<(), DeviceError>> + Send;

    /// Delete a device.
    fn delete(&self, id: &ClientId) -> impl Future<Output = Result<(), DeviceError>> + Send;
}
