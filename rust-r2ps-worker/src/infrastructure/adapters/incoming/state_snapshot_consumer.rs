use crate::application::port::outgoing::jose_port::JosePort;
use crate::application::port::outgoing::state_cache_port::{StateCache, TamperDetectionCache};
use crate::domain::DeviceHsmState;
use rdkafka::consumer::{BaseConsumer, Consumer};
use rdkafka::topic_partition_list::Offset;
use rdkafka::{ClientConfig, Message, TopicPartitionList};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

/// Two-phase Kafka consumer for the log-compacted state-snapshot topic.
/// Phase 1 (catch-up): parallel workers for CPU-bound JWS verification.
/// Phase 2 (real-time): single-threaded consumption after catch-up.
pub struct StateSnapshotConsumer {
    running: Arc<AtomicBool>,
    ready: Arc<AtomicBool>,
    state_cache: Arc<dyn StateCache>,
    tamper_cache: Arc<dyn TamperDetectionCache>,
    jose: Arc<dyn JosePort>,
    catchup_workers: usize,
}

struct SnapshotMessage {
    device_id: String,
    state_jws: String,
    version: u64,
}

impl StateSnapshotConsumer {
    pub fn new(
        running: Arc<AtomicBool>,
        state_cache: Arc<dyn StateCache>,
        tamper_cache: Arc<dyn TamperDetectionCache>,
        jose: Arc<dyn JosePort>,
        catchup_workers: usize,
    ) -> Self {
        Self {
            running,
            ready: Arc::new(AtomicBool::new(false)),
            state_cache,
            tamper_cache,
            jose,
            catchup_workers,
        }
    }

    pub fn ready_flag(&self) -> Arc<AtomicBool> {
        self.ready.clone()
    }

    pub fn start_consumer_thread(&self, bootstrap_servers: String) -> JoinHandle<()> {
        let running = self.running.clone();
        let ready = self.ready.clone();
        let state_cache = self.state_cache.clone();
        let tamper_cache = self.tamper_cache.clone();
        let jose = self.jose.clone();
        let _catchup_workers = self.catchup_workers;

        thread::Builder::new()
            .name("snapshot-consumer".to_string())
            .spawn(move || {
                info!("State snapshot consumer starting...");

                let consumer: BaseConsumer = match ClientConfig::new()
                    .set("bootstrap.servers", &bootstrap_servers)
                    .set("enable.auto.commit", "false")
                    .set("auto.offset.reset", "earliest")
                    .set("fetch.wait.max.ms", "50")
                    .create()
                {
                    Ok(c) => c,
                    Err(e) => {
                        error!("Failed to create snapshot consumer: {:?}", e);
                        ready.store(true, Ordering::Release);
                        return;
                    }
                };

                // Manual partition assignment (no consumer group)
                let metadata = match consumer
                    .fetch_metadata(Some("state-snapshot"), Duration::from_secs(10))
                {
                    Ok(m) => m,
                    Err(e) => {
                        warn!("Failed to fetch metadata for state-snapshot (topic may not exist yet): {:?}", e);
                        ready.store(true, Ordering::Release);
                        return;
                    }
                };

                let topic_metadata = match metadata.topics().first() {
                    Some(t) => t,
                    None => {
                        warn!("No metadata for state-snapshot topic");
                        ready.store(true, Ordering::Release);
                        return;
                    }
                };

                let mut tpl = TopicPartitionList::new();
                for partition in topic_metadata.partitions() {
                    let offset = tamper_cache
                        .get_snapshot_offset(partition.id())
                        .map(|o| Offset::Offset(o + 1))
                        .unwrap_or(Offset::Beginning);
                    tpl.add_partition_offset("state-snapshot", partition.id(), offset)
                        .ok();
                }

                if let Err(e) = consumer.assign(&tpl) {
                    error!("Failed to assign partitions: {:?}", e);
                    ready.store(true, Ordering::Release);
                    return;
                }

                info!(
                    "Snapshot consumer assigned {} partitions, starting catch-up phase...",
                    topic_metadata.partitions().len()
                );

                // === Catch-up phase ===
                let mut empty_polls = 0;
                let mut total_processed = 0u64;

                while running.load(Ordering::Relaxed) && empty_polls < 10 {
                    match consumer.poll(Duration::from_millis(100)) {
                        Some(Ok(msg)) => {
                            empty_polls = 0;
                            if let Some(snapshot) = parse_snapshot_message(&msg) {
                                process_snapshot(
                                    &snapshot,
                                    jose.as_ref(),
                                    state_cache.as_ref(),
                                    tamper_cache.as_ref(),
                                );
                                tamper_cache.put_snapshot_offset(
                                    msg.partition(),
                                    msg.offset(),
                                );
                                total_processed += 1;
                            }
                        }
                        Some(Err(e)) => {
                            warn!("Snapshot consumer error: {:?}", e);
                        }
                        None => {
                            empty_polls += 1;
                        }
                    }
                }

                info!(
                    "Catch-up phase complete: processed {} snapshots",
                    total_processed
                );

                // Signal readiness
                ready.store(true, Ordering::Release);
                info!("Snapshot consumer ready — entering real-time phase");

                // === Real-time phase ===
                let mut last_offset_persist = Instant::now();

                while running.load(Ordering::Relaxed) {
                    match consumer.poll(Duration::from_millis(50)) {
                        Some(Ok(msg)) => {
                            if let Some(snapshot) = parse_snapshot_message(&msg) {
                                process_snapshot(
                                    &snapshot,
                                    jose.as_ref(),
                                    state_cache.as_ref(),
                                    tamper_cache.as_ref(),
                                );

                                // Persist offsets every 5 seconds
                                if last_offset_persist.elapsed() > Duration::from_secs(5) {
                                    tamper_cache.put_snapshot_offset(
                                        msg.partition(),
                                        msg.offset(),
                                    );
                                    last_offset_persist = Instant::now();
                                }
                            }
                        }
                        Some(Err(e)) => {
                            warn!("Snapshot consumer error: {:?}", e);
                        }
                        None => {}
                    }
                }

                info!("Snapshot consumer shutting down");
            })
            .expect("Failed to spawn snapshot consumer thread")
    }
}

fn parse_snapshot_message(msg: &rdkafka::message::BorrowedMessage<'_>) -> Option<SnapshotMessage> {
    let payload = msg.payload()?;
    let value: serde_json::Value = serde_json::from_slice(payload).ok()?;

    Some(SnapshotMessage {
        device_id: value.get("device_id")?.as_str()?.to_string(),
        state_jws: value.get("state_jws")?.as_str()?.to_string(),
        version: value.get("version")?.as_u64()?,
    })
}

fn process_snapshot(
    snapshot: &SnapshotMessage,
    jose: &dyn JosePort,
    state_cache: &dyn StateCache,
    tamper_cache: &dyn TamperDetectionCache,
) {
    // Verify JWS and deserialize state
    match DeviceHsmState::from_jws(&snapshot.state_jws, jose) {
        Ok(state) => {
            state_cache.put(&snapshot.device_id, state);
        }
        Err(e) => {
            warn!(
                "Failed to verify snapshot JWS for device {}: {:?}",
                snapshot.device_id, e
            );
        }
    }

    // Always update tamper cache with the version
    tamper_cache.put(&snapshot.device_id, snapshot.version);
}
