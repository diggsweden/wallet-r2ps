use crate::domain::device_management::value_objects::{ClientId, DeviceState};

/// Aggregate root for the Device Management bounded context.
///
/// A `Device` represents a registered wallet device that holds cryptographic state
/// managed by the HSM worker. The device state is opaque (JWS-signed) and is
/// updated when the HSM worker processes requests.
#[derive(Debug, Clone)]
pub struct Device {
    id: ClientId,
    state: DeviceState,
}

impl Device {
    /// Create a new device with the given identity and initial state.
    pub fn new(id: ClientId, state: DeviceState) -> Self {
        Self { id, state }
    }

    /// Return the device identifier.
    pub fn id(&self) -> &ClientId {
        &self.id
    }

    /// Return the current device state.
    pub fn state(&self) -> &DeviceState {
        &self.state
    }

    /// Update the device state.
    ///
    /// Called when the HSM worker returns a new state blob after processing
    /// a service request.
    pub fn update_state(&mut self, new_state: DeviceState) {
        self.state = new_state;
    }

    /// Consume the device and return its parts.
    pub fn into_parts(self) -> (ClientId, DeviceState) {
        (self.id, self.state)
    }
}
