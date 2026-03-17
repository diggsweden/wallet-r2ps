use postgres::fallible_iterator::FallibleIterator;
use postgres::{Client, NoTls};
use rdkafka::producer::{BaseProducer, BaseRecord, Producer};
use rdkafka::ClientConfig;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tracing::{debug, error, info, warn};

const OUTBOX_CHANNEL: &str = "outbox_channel";

/// Standalone thread that listens for PostgreSQL NOTIFY events on the outbox
/// table and publishes pending entries to Kafka.
///
/// Falls back to a periodic safety poll (configurable via `poll_timeout`) to
/// catch any notifications missed during reconnects or PG restarts.
pub struct OutboxRelay {
    running: Arc<AtomicBool>,
}

impl OutboxRelay {
    pub fn new(running: Arc<AtomicBool>) -> Self {
        Self { running }
    }

    pub fn start_relay_thread(
        &self,
        connection_string: String,
        bootstrap_servers: String,
        poll_timeout: Duration,
    ) -> JoinHandle<()> {
        let running = self.running.clone();

        thread::Builder::new()
            .name("outbox-relay".to_string())
            .spawn(move || {
                info!("Outbox relay thread starting...");

                let mut pg_client = match Self::connect_and_listen(&connection_string) {
                    Ok(c) => c,
                    Err(e) => {
                        error!("Outbox relay: failed to connect to PostgreSQL: {}", e);
                        return;
                    }
                };

                let producer: BaseProducer = match ClientConfig::new()
                    .set("bootstrap.servers", &bootstrap_servers)
                    .set("acks", "1")
                    .set("linger.ms", "0")
                    .set("socket.nagle.disable", "true")
                    .create()
                {
                    Ok(p) => p,
                    Err(e) => {
                        error!("Outbox relay: failed to create Kafka producer: {:?}", e);
                        return;
                    }
                };

                info!(
                    "Outbox relay started (LISTEN/NOTIFY mode, safety timeout={:?})",
                    poll_timeout
                );

                while running.load(Ordering::Relaxed) {
                    // Phase 1: Drain all pending outbox entries in batches
                    match Self::drain_outbox(&mut pg_client, &producer) {
                        Ok(total) => {
                            if total > 0 {
                                debug!("Outbox relay: published {} entries total", total);
                                // Cleanup old published rows after a successful drain
                                Self::cleanup_published(&mut pg_client);
                            }
                        }
                        Err(e) => {
                            warn!("Outbox relay error: {}", e);
                            Self::reconnect(&mut pg_client, &connection_string);
                            continue;
                        }
                    }

                    // Phase 2: Wait for NOTIFY or safety-poll timeout
                    // This blocks the thread on the PG socket — no busy-waiting.
                    // Wakes up when:
                    //   a) A NOTIFY arrives (new outbox rows committed), or
                    //   b) The timeout expires (safety poll to catch missed events)
                    let _ = pg_client.notifications().timeout_iter(poll_timeout).next();
                }

                info!("Outbox relay thread shutting down");
            })
            .expect("Failed to spawn outbox relay thread")
    }

    /// Connect to PostgreSQL and subscribe to the outbox notification channel.
    fn connect_and_listen(connection_string: &str) -> Result<Client, String> {
        let mut client = Client::connect(connection_string, NoTls)
            .map_err(|e| format!("PostgreSQL connection failed: {}", e))?;

        client
            .execute(&format!("LISTEN {}", OUTBOX_CHANNEL), &[])
            .map_err(|e| format!("LISTEN {} failed: {}", OUTBOX_CHANNEL, e))?;

        info!(
            "Connected to PostgreSQL and subscribed to LISTEN {}",
            OUTBOX_CHANNEL
        );
        Ok(client)
    }

    /// Reconnect and re-subscribe to the LISTEN channel.
    fn reconnect(pg_client: &mut Client, connection_string: &str) {
        thread::sleep(Duration::from_secs(1));
        match Self::connect_and_listen(connection_string) {
            Ok(c) => {
                *pg_client = c;
            }
            Err(e) => {
                error!("Outbox relay: reconnection failed: {}", e);
            }
        }
    }

    /// Drain all unpublished outbox entries in batches of 100 until empty.
    fn drain_outbox(pg_client: &mut Client, producer: &BaseProducer) -> Result<usize, String> {
        let mut total = 0;
        loop {
            let count = Self::poll_and_publish(pg_client, producer)?;
            total += count;
            if count == 0 {
                break;
            }
        }
        Ok(total)
    }

    /// Fetch one batch of unpublished entries, publish to Kafka, mark as published.
    ///
    /// Uses `FOR UPDATE SKIP LOCKED` so that multiple pods can run the outbox
    /// relay concurrently without processing the same rows. Each pod locks and
    /// processes a disjoint set of entries. The SELECT and UPDATE happen within
    /// a single transaction to hold the row locks until entries are marked published.
    fn poll_and_publish(pg_client: &mut Client, producer: &BaseProducer) -> Result<usize, String> {
        let mut tx = pg_client
            .transaction()
            .map_err(|e| format!("begin transaction failed: {}", e))?;

        // Fetch unpublished entries, locking them to prevent other pods from processing
        let rows = tx
            .query(
                "SELECT id, topic, key, payload FROM outbox
                 WHERE NOT published
                 ORDER BY id
                 LIMIT 100
                 FOR UPDATE SKIP LOCKED",
                &[],
            )
            .map_err(|e| format!("query outbox failed: {}", e))?;

        if rows.is_empty() {
            // Nothing to process — commit (releases any locks) and return
            tx.commit().map_err(|e| format!("commit failed: {}", e))?;
            return Ok(0);
        }

        let mut published_ids: Vec<i64> = Vec::with_capacity(rows.len());

        for row in &rows {
            let id: i64 = row.get(0);
            let topic: String = row.get(1);
            let key: String = row.get(2);
            let payload: serde_json::Value = row.get(3);

            let payload_bytes = serde_json::to_vec(&payload)
                .map_err(|e| format!("serialize outbox payload failed: {}", e))?;

            match producer.send(BaseRecord::to(&topic).key(&key).payload(&payload_bytes)) {
                Ok(()) => {
                    published_ids.push(id);
                }
                Err((e, _)) => {
                    warn!("Outbox relay: failed to send to Kafka: {:?}", e);
                    break;
                }
            }
        }

        // Flush to ensure delivery before marking as published
        producer
            .flush(Duration::from_secs(5))
            .map_err(|e| format!("flush failed: {:?}", e))?;

        // Mark as published within the same transaction
        if !published_ids.is_empty() {
            let ids_str: Vec<String> = published_ids.iter().map(|id| id.to_string()).collect();
            let query = format!(
                "UPDATE outbox SET published = true WHERE id IN ({})",
                ids_str.join(",")
            );
            tx.execute(&query, &[])
                .map_err(|e| format!("update outbox published failed: {}", e))?;
        }

        tx.commit().map_err(|e| format!("commit failed: {}", e))?;

        Ok(published_ids.len())
    }

    /// Delete published outbox rows older than 5 minutes to prevent unbounded table growth.
    fn cleanup_published(pg_client: &mut Client) {
        match pg_client.execute(
            "DELETE FROM outbox WHERE published = true AND created_at < now() - interval '5 minutes'",
            &[],
        ) {
            Ok(count) if count > 0 => debug!("Outbox cleanup: deleted {} old rows", count),
            Ok(_) => {}
            Err(e) => warn!("Outbox cleanup failed: {}", e),
        }
    }
}
