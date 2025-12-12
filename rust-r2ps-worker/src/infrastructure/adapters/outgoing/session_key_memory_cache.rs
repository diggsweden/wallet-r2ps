use std::sync::Arc;
use std::time::Duration;
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use dotenv_codegen::dotenv;
use tracing::{error, info};
use crate::application::client_repository_spi_port::{ClientRepositoryError, ClientRepositorySpiPort};
use crate::application::{load_pem_from_bas64_env, LoadPemError};
use crate::application::session_key_spi_port::{LoginSession, LoginState, SessionKey, SessionKeySpiPort};
use crate::domain::{ClientMetadata, DefaultCipherSuite};
use moka::sync::Cache;
use opaque_ke::ServerLoginStartResult;

pub struct SessionKeyMemoryCache {
    cache: Cache<String, SessionKey>,
    start_auth: Cache<String, Arc<LoginSession>>,

}

impl SessionKeyMemoryCache {
    pub fn new() -> SessionKeyMemoryCache {

        let cache = Cache::builder()
            .time_to_live(Duration::from_secs(600)) // TODO config
            .max_capacity(10_000)// TODO
            .build();

        let start_auth = Cache::builder()
            .time_to_live(Duration::from_secs(600)) // TODO config
            .max_capacity(10_000)// TODO
            .build();


        SessionKeyMemoryCache {
            cache,
            start_auth
        }
    }
}

impl SessionKeySpiPort for SessionKeyMemoryCache {

    fn store(&self, pake_session_id: &str, session_key: &SessionKey) -> Result<(), crate::application::session_key_spi_port::ClientRepositoryError> {
        info!("storing session key session_id: {} {:02X?}", pake_session_id, session_key);
        self.cache.insert(pake_session_id.to_string(), session_key.clone() );
        Ok(())
    }

    fn get(&self, pake_session_id: &str) -> Option<SessionKey> {
        info!("get session key session_id: {}", pake_session_id);

        match self.cache.get(pake_session_id) {
            Some(session_key) => {
                info!("get session key session_id: {} {:02X?}", pake_session_id, session_key);

                Some(session_key)
            },
            None => None
        }
    }

    fn end_session(&self, pake_session_id: &str) -> Result<(), crate::application::session_key_spi_port::ClientRepositoryError> {
        match self.cache.remove(pake_session_id) {
            None => Ok(()),
            Some(_) => Ok(())
        }
    }

    fn store_pending_auth(&self, client_id: &str, server_login_start_result: &Arc<LoginSession>) {
        self.start_auth.insert(client_id.to_string(), server_login_start_result.clone());
    }

    fn get_pending_auth(&self, client_id: &str) -> Option<Arc<LoginSession>> {
        match self.start_auth.remove(client_id) {
            Some(session) => {
               Some(session)
            },
            None => None
        }
    }
}