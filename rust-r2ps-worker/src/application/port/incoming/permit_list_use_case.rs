use josekit::jwk::Jwk;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// Wallet app backend can register valid public keys for a device
/// The key can be added to permit list (allow/deny)
///
pub trait PermitListUseCase<DeviceRegistrationError> {
    fn register_device(
        &self,
        server_wallet_id: &ServerWalletId,
        device_id: &DeviceId,
        registration: &PermitListDto,
    ) -> Result<(), DeviceRegistrationError>;
}

pub type DeviceId = Uuid;
pub type ServerWalletId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PermitListDto {
    #[schema(value_type = String, format = "uuid")]
    pub device_id: DeviceId,
    #[schema(value_type = String, format = "uuid")]
    pub server_wallet_id: ServerWalletId,
    pub device_keys: PermitStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub enum PermitStatus {
    Allow(DeviceKey),
    Deny(DeviceKey),
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeviceKey {
    #[schema(value_type = Object)] // Jwk has not implemented ToSchema
    pub device_public_key: Jwk,
}

/*

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RegisterNewDeviceDto {
    pub device_id: String,
    #[schema(value_type = Object)] // Jwk has not implemented ToSchema
    pub device_public_key: Jwk
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CreateNewServerWalletKeyDto {
    #[schema(value_type = String, format = "uuid")]
    pub server_wallet_id: ServerWalletId,
    pub curve: Curve,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RevokeDeviceDto {
    #[schema(value_type = String, format = "uuid")]
    pub server_wallet_id: ServerWalletId,
    pub device_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RevokeServerWalletDto {
    pub server_wallet_id: String,
}

#[derive(Debug, Clone, ToSchema, Serialize, Deserialize)]
pub enum RegistrationPayload {
    RegisterNewDevice(RegisterNewDeviceDto),
    CreateNewServerWalletKey(CreateNewServerWalletKeyDto),
    RevokeDeviceKey(RevokeDeviceDto),
    RevokeServerWallet(RevokeServerWalletDto),
}
*/
