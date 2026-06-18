// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Idempotent topic creation. The k3s Kafka cluster runs with
//! `auto.create.topics.enable=false`, so the load-test must pre-create the
//! per-process response topics before any client uses them.

use anyhow::{Context, Result};
use rdkafka::admin::{AdminClient, AdminOptions, NewTopic, TopicReplication};
use rdkafka::client::DefaultClientContext;
use rdkafka::ClientConfig;
use std::time::Duration;
use tracing::{info, warn};

/// Create `topics` if they don't already exist. `TopicAlreadyExists` is
/// treated as success.
pub async fn ensure_topics(
    bootstrap_servers: &str,
    broker_address_family: &str,
    topics: &[&str],
    partitions: i32,
    replication: i32,
) -> Result<()> {
    let admin: AdminClient<DefaultClientContext> = ClientConfig::new()
        .set("bootstrap.servers", bootstrap_servers)
        .set("broker.address.family", broker_address_family)
        .create()
        .context("Failed to build AdminClient")?;

    let new_topics: Vec<NewTopic<'_>> = topics
        .iter()
        .map(|name| {
            NewTopic::new(name, partitions, TopicReplication::Fixed(replication))
                // 5-minute retention is plenty — responses are consumed within seconds.
                .set("retention.ms", "300000")
                .set("cleanup.policy", "delete")
        })
        .collect();

    let results = admin
        .create_topics(&new_topics, &AdminOptions::new().request_timeout(Some(Duration::from_secs(15))))
        .await
        .context("create_topics failed")?;

    for r in results {
        match r {
            Ok(name) => info!("Created Kafka topic: {}", name),
            Err((name, err)) => {
                if matches!(err, rdkafka::types::RDKafkaErrorCode::TopicAlreadyExists) {
                    warn!("Topic {} already exists, reusing", name);
                } else {
                    anyhow::bail!("Failed to create topic {}: {:?}", name, err);
                }
            }
        }
    }

    Ok(())
}
