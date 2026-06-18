<!--
SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government

SPDX-License-Identifier: EUPL-1.2
-->

# Wallet BFF

Axum-based REST API (BFF). Every request is one Kafka message: the POST returns
`202 Accepted` immediately with a `Location` header, and a GET on that URL
long-polls (Redis Pub/Sub) until the worker's response is available. Any BFF
replica can serve the GET regardless of which replica produced the request,
because both the request context (`req-ctx:{id}`) and the response envelope
(`hsm-response:{id}`, `state-init-response:{id}`) live in Redis/Valkey.

Endpoints:

- `POST /hsm/v1/device-states`      — enqueue state-init, returns 202 + Location
- `GET  /hsm/v1/device-states/{id}` — long-poll for state-init result
- `POST /hsm/v1/requests`           — enqueue worker request, returns 202 + Location
- `GET  /hsm/v1/requests/{id}`      — long-poll for worker result

## Dev tools

Install rust toolchain from [rustup](https://rustup.rs/).

Install the following rust-rdkafka
dependencies ([rdkafka installation instructions](https://github.com/fede1024/rust-rdkafka?tab=readme-ov-file#installation))
to build the project locally:

### Debian/Ubuntu

```bash
apt-get update && apt-get install -y \
    zlib1g zlib1g-dev \
    cmake \
    libssl-dev \
    libsasl2-dev \
    libzstd-dev
```

### OSX

```bash
brew install \
    zlib \
    cmake \
    openssl \
    cyrus-sasl \
    zstd
```

## Build and run

```bash
cargo run
```

## Testing

### Unit tests

```bash
cargo test
```

### Integration tests (Tier 2)

Integration tests use [testcontainers](https://crates.io/crates/testcontainers) to spin up real Kafka and Redis/Valkey containers. They are gated with `#[ignore]` and require Docker.

```bash
# Run all integration tests (serial — shared topic names)
cargo test -- --ignored --test-threads=1

# Run a single integration test
cargo test -- --ignored test_device_state_redis_round_trip

# Run all tests (unit + integration) in one go
cargo test -- --include-ignored --test-threads=1
```
