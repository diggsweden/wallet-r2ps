use crate::application::ClientRepositorySpiPort;
use crate::application::device_permit_list_spi_port::{DevicePermitListSpiPort, PermitListError};
use crate::application::permit_list_use_case::{DeviceId, PermitListDto};
use std::sync::Arc;

#[derive(Clone)]
pub struct DeviceMetadataService {
    client_repository_spi_port: Arc<dyn ClientRepositorySpiPort + Send + Sync>,
    device_permit_list_spi_port: Arc<dyn DevicePermitListSpiPort + Send + Sync>,
}

impl DeviceMetadataService {
    pub fn new(
        client_repository_spi_port: Arc<dyn ClientRepositorySpiPort + Send + Sync>,
        device_permit_list_spi_port: Arc<dyn DevicePermitListSpiPort + Send + Sync>,
    ) -> Self {
        Self {
            client_repository_spi_port,
            device_permit_list_spi_port,
        }
    }

    pub fn update_device_permit_list(
        &self,
        device_id: DeviceId,
        permit_list_dto: PermitListDto,
    ) -> Result<(), PermitListError> {
        self.device_permit_list_spi_port
            .store_permit_list(device_id, permit_list_dto)
            .map_err(|e| e.into())
    }
}
