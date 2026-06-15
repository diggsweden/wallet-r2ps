// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

#[derive(Debug, Clone)]
pub struct KafkaConfig {
    pub bootstrap_servers: String,
    pub broker_address_family: String,
    pub group_id: String,
    pub group_instance_id: String,
    /// Number of worker threads draining the request dispatch channels.
    /// Each worker owns a stable subset of partitions via partition_id % N.
    pub request_worker_tasks: usize,
    /// Per-worker bounded channel depth for the request dispatch path.
    /// Caps in-flight messages per worker; provides back-pressure.
    pub request_worker_queue_depth: usize,
    /// Number of independent BaseConsumer poll loops feeding the
    /// shared request dispatch channel.
    pub request_num_consumers: usize,
    /// Number of worker threads draining the state-init dispatch channels.
    pub state_init_worker_tasks: usize,
    /// Per-worker bounded channel depth for the state-init dispatch path.
    pub state_init_worker_queue_depth: usize,
    /// Producer `linger.ms` for the response producers. Same env var
    /// (KAFKA_PRODUCER_LINGER_MS) as the BFF side so they can be tuned
    /// together. See response producer modules for the trade-off.
    pub producer_linger_ms: u64,
}
