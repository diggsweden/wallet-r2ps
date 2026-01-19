use crate::application::client_repository_spi_port::{
    ClientRepositoryError, ClientRepositorySpiPort,
};
use crate::application::load_pem_from_bas64_env;
use crate::domain::ClientMetadata;
use crate::domain::HsmKey;
use foyer::{Cache, CacheBuilder, EvictionConfig, LruConfig};
use tracing::error;

pub struct ClientRepositoryMemoryCache {
    cache: Cache<String, ClientMetadata>,
}

impl Default for ClientRepositoryMemoryCache {
    fn default() -> Self {
        Self::new()
    }
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
        self.cache.get(client_id).map(|elem| elem.value().clone())
    }

    fn store_metadata(&self, client_metadata: ClientMetadata) -> Result<(), ClientRepositoryError> {
        self.cache
            .insert(client_metadata.client_id.clone(), client_metadata);
        Ok(())
    }

    fn find_key(&self, client_id: &str, kid: &str) -> Result<HsmKey, ClientRepositoryError> {
        let client_metadata = self
            .client_metadata(client_id)
            .ok_or(ClientRepositoryError::ClientNotFound)?;

        client_metadata
            .keys
            .iter()
            .find(|&key| key.kid().eq(kid))
            .cloned()
            .ok_or(ClientRepositoryError::KeyNotFound)
    }

    fn add_key(&self, client_id: &str, key: &HsmKey) -> Result<(), ClientRepositoryError> {
        // TODO race condition - just for demo
        let mut metadata = self
            .client_metadata(client_id)
            .ok_or(ClientRepositoryError::ClientNotFound)?;
        metadata.keys.push(key.clone());
        self.store_metadata(metadata)?;
        Ok(())
    }

    fn delete_key(&self, client_id: &str, kid: &str) -> Result<(), ClientRepositoryError> {
        let mut metadata = self
            .client_metadata(client_id)
            .ok_or(ClientRepositoryError::ClientNotFound)?;
        metadata.keys.retain(|key| key.kid() != kid);
        self.store_metadata(metadata)?;
        Ok(())
    }
}
