pub trait SessionKeySpiPort {
    fn store(
        &self,
        pake_session_id: &str,
        session_key: &SessionKey,
    ) -> Result<(), ClientRepositoryError>;
    fn get(&self, pake_session_id: &str) -> Option<SessionKey>;
    fn end_session(&self, pake_session_id: &str) -> Result<(), ClientRepositoryError>;
}

#[derive(Debug)]
pub enum ClientRepositoryError {
    Unknown,
}

pub type SessionKey = Vec<u8>;
