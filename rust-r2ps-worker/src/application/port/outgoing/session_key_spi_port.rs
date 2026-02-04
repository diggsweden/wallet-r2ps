use crate::{define_byte_vector, domain::SessionId};
use std::time::Duration;

define_byte_vector!(SessionKey);

pub trait SessionKeySpiPort {
    fn store(
        &self,
        id: &SessionId,
        session_key: SessionKey,
    ) -> Result<Duration, ClientRepositoryError>;
    fn get(&self, id: &SessionId) -> Option<SessionKey>;
    fn get_remaining_ttl(&self, id: &SessionId) -> Option<Duration>;
    fn end_session(&self, id: &SessionId) -> Result<(), ClientRepositoryError>;
}

#[derive(Debug)]
pub enum ClientRepositoryError {
    Unknown,
}
