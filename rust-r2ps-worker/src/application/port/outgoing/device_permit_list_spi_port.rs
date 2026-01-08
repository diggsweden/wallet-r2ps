use crate::application::permit_list_use_case::{DeviceId, PermitListDto};

pub trait DevicePermitListSpiPort {
    fn store_permit_list(
        &self,
        device_id: DeviceId,
        permit_list_item: PermitListDto,
    ) -> Result<(), PermitListError>;
    fn get_permit_list(&self, device_id: DeviceId) -> Option<PermitListDto>;
}

#[derive(Debug)]
pub enum PermitListError {
    Unknown,
}
