use crate::application::client_repository_spi_port::{
    ClientRepositoryError, ClientRepositorySpiPort,
};
use crate::application::load_pem_from_bas64_env;
use crate::domain::ClientMetadata;
use crate::infrastructure::hsm_wrapper::HsmKey;
use foyer::{Cache, CacheBuilder, EvictionConfig, LruConfig};
use tracing::error;

pub struct ClientRepositoryMemoryCache {
    cache: Cache<String, ClientMetadata>,
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
                cache.insert(
                    "a25d8884-c77b-43ab-bf9d-1279c08d860d".to_string(),
                    ClientMetadata {
                        client_id: "a25d8884-c77b-43ab-bf9d-1279c08d860d".to_string(),
                        wallet_id: "a25d8884-c77b-43ab-bf9d-1279c08d860d".to_string(),
                        client_public_key,
                        password_file: None,
                        keys: Vec::new(),
                    },
                );
            }
            Err(e) => {
                error!("Invalid CLIENT_PUBLIC_KEY env variable value: {:?}", e);
            }
        }

        ClientRepositoryMemoryCache { cache }
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
        self.cache
            .insert(client_metadata.client_id.clone(), client_metadata);
        Ok(())
    }

    fn add_key(&self, client_id: &str, key: &HsmKey) -> Result<(), ClientRepositoryError> {
        // TODO race condition - just for demo
        match self.client_metadata(client_id) {
            None => Ok(()),
            Some(metadata) => {
                let new_metadata = ClientMetadata {
                    keys: {
                        let mut new_keys = metadata.keys.clone();
                        new_keys.push(key.clone());
                        new_keys
                    },
                    ..metadata
                };
                self.store_metadata(new_metadata)?;
                Ok(())
            }
        }
    }
}
