// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

#[derive(Debug, Clone)]
pub struct KafkaConfig {
    pub bootstrap_servers: String,
    pub broker_address_family: String,
    pub group_id: String,
    pub group_instance_id: String,
    pub consumer_threads_request: usize,
    pub consumer_threads_state_init: usize,
}
