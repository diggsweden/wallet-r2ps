use redis::aio::ConnectionManager;

/// Shared Redis connection used by all Redis-based adapters.
#[derive(Clone)]
pub struct RedisConnection {
    conn: ConnectionManager,
}

impl RedisConnection {
    pub async fn new(redis_url: &str) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(redis_url)?;
        let conn = ConnectionManager::new(client).await?;
        Ok(Self { conn })
    }

    pub fn inner(&self) -> &ConnectionManager {
        &self.conn
    }

    /// Get a clone of the connection manager (needed for mutable Redis commands).
    pub fn get(&self) -> ConnectionManager {
        self.conn.clone()
    }
}
