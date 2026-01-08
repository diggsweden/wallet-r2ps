use crate::application::device_permit_list_spi_port::{DevicePermitListSpiPort, PermitListError};
use crate::application::permit_list_use_case::{DeviceId, PermitListDto};
use moka::sync::Cache;
use std::time::Duration;
use tracing::info;

pub struct DevicePermitListMemoryCache {
    permit_list: Cache<String, PermitListDto>,
}

impl DevicePermitListMemoryCache {
    pub fn new() -> DevicePermitListMemoryCache {
        let permit_list = Cache::builder()
            .time_to_live(Duration::from_secs(600)) // TODO config
            .max_capacity(10_000) // TODO
            .build();

        Self { permit_list }
    }
}

impl DevicePermitListSpiPort for DevicePermitListMemoryCache {
    fn store_permit_list(
        &self,
        device_id: DeviceId,
        permit_list_item: PermitListDto,
    ) -> Result<(), PermitListError> {
        info!("storing item to permit list device_id: {}", device_id);
        self.permit_list
            .insert(device_id.to_string(), permit_list_item.clone());
        Ok(())
    }

    fn get_permit_list(&self, device_id: DeviceId) -> Option<PermitListDto> {
        info!("get permit list item for device_id: {}", device_id);

        match self.permit_list.get(&device_id.to_string()) {
            Some(permit_item) => {
                info!("found permit list item for device_id: {}", device_id);

                Some(permit_item)
            }
            None => None,
        }
    }
}
