use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use dotenv_codegen::dotenv;
use foyer::{Cache, CacheBuilder, EvictionConfig, LruConfig};
use pem::Pem;
use tracing::error;
use crate::application::client_repository_spi_port::{ClientRepositoryError, ClientRepositorySpiPort};
use crate::application::{load_pem_from_bas64_env, LoadPemError};
use crate::domain::ClientMetadata;

pub struct ClientRepositoryMemoryCache {
    cache: Cache<String, ClientMetadata>
}

impl ClientRepositoryMemoryCache {
    pub fn new() -> ClientRepositoryMemoryCache {

        let cache: Cache<String, ClientMetadata> = CacheBuilder::new(2048)
            .with_eviction_config(EvictionConfig::Lru(LruConfig {
                high_priority_pool_ratio: 0.8,
            }))
            .build();


        match load_pem_from_bas64_env("CLIENT_PUBLIC_KEY") {
            Ok(client_public_key) => {
                cache.insert("a25d8884-c77b-43ab-bf9d-1279c08d860d".to_string(), ClientMetadata {
                    client_id: "a25d8884-c77b-43ab-bf9d-1279c08d860d".to_string(),
                    wallet_id: "a25d8884-c77b-43ab-bf9d-1279c08d860d".to_string(),
                    client_public_key,
                    password_file: None,
                });
            }
            Err(e) => {
                error!("Invalid CLIENT_PUBLIC_KEY env variable value: {:?}", e);
            }
        }
        

        ClientRepositoryMemoryCache {
            cache
        }
    }
}

impl ClientRepositorySpiPort for ClientRepositoryMemoryCache {
    fn client_metadata(&self, client_id: &str) -> Option<ClientMetadata> {
        match self.cache.get(client_id) {
            Some(elem) => Some(elem.value().clone()),
            None => None,
        }
    }

    fn store_metadata(&self, client_metadata: ClientMetadata) -> Result<(), ClientRepositoryError> {
        self.cache.insert(client_metadata.client_id.clone(), client_metadata);
        Ok(())
    }
}