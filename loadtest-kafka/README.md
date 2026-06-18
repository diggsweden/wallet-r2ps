<!--
SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government

SPDX-License-Identifier: EUPL-1.2
-->

# loadtest-kafka

Direct-to-Kafka load tester for the R2PS wallet stack. Sibling crate of
`integration-load-tests` — same OPAQUE + JWS/JWE code paths, but every
round-trip is a Kafka produce + consume instead of an HTTP call to the BFF.

Use this when you want to isolate the worker pipeline (hsm-requests →
hsm-worker → response topic) from the BFF and from any HTTP/long-poll overhead.

```
        load-test process                       cluster
   ┌───────────────────────┐
   │ VU0  VU1 … VUn  (tokio │ ── hsm-requests ─────►  hsm-worker pool
   │                  tasks)│ ── state-init-requests ►
   │           │            │
   │  KafkaBackend          │ ◄── loadtest-hsm-responses-<uuid>      │
   │  ├─ FutureProducer     │ ◄── loadtest-state-init-responses-<uuid>
   │  └─ StreamConsumer ──► │
   │      DashMap<id, oneshot>          (per-request correlation)    │
   └───────────────────────┘
```

## Architecture

* **One shared `FutureProducer`.** `rdkafka` is internally multiplexed; every
  VU enqueues to the same producer with negligible contention.
* **Per-process response topics** — `loadtest-hsm-responses-<uuid>` and
  `loadtest-state-init-responses-<uuid>` — created on startup via
  `AdminClient`. Each request envelope carries its `response_topic` so the
  worker routes the reply back to *this* process. Multiple load-test
  processes coexist without crosstalk.
* **One `StreamConsumer` per response topic.** Each runs as a dedicated
  Tokio task, parses incoming messages, looks up `requestId` in a
  `DashMap<String, oneshot::Sender>`, and resolves the matching waiter.
  Per-VU code only sees a `oneshot::Receiver`, never the consumer.
* **Per-client state cache.** The `hsm-worker` requires the latest
  `state_jws` on every request. The backend caches it (keyed by
  `client_id`) and updates it on every successful response. The `generate`
  command persists the final `state_jws` into the dataset so the
  `load-test` command can re-seed without redoing onboarding.
* **Client partitioning across VUs.** VU *i* only touches clients
  `i, i+threads, i+2*threads, …`. No two VUs ever own the same
  `client_id`, so the per-client state cache is mutation-safe without
  locks.

## Kafka tuning

Producer (`KafkaBackend::build_producer`):

| key | value | why |
| --- | --- | --- |
| `linger.ms` | `2` | match in-cluster BFF; coalesces bursts without holding sparse traffic |
| `batch.size` | `131072` | 128 KiB — large enough to absorb VU bursts |
| `compression.type` | `lz4` | small CPU cost, large network/disk savings |
| `acks` | `1` | leader-only — durability isn't under test |
| `enable.idempotence` | `false` | uncorrelated to throughput here |
| `queue.buffering.max.messages` | `1000000` | no producer-side backpressure on VU loop |
| `queue.buffering.max.kbytes` | `262144` | matches above |

Consumer (`spawn_*_response_reader`):

| key | value | why |
| --- | --- | --- |
| `fetch.wait.max.ms` | `10` | low-latency: drain as soon as the broker has anything |
| `fetch.min.bytes` | `1` | as above |
| `enable.auto.commit` | `false` | no need; we never re-read the topic |
| `auto.offset.reset` | `latest` | topics are fresh per process |
| `session.timeout.ms` | `10000` | fast detection if we die mid-test |

## Commands

### `generate` — onboard clients via Kafka

```bash
loadtest-kafka generate \
  --bootstrap-servers access-kafka-cluster-kafka-bootstrap:9092 \
  --server-pubkey-pem /path/to/server-pubkey.pem \
  -n 100 -c 16 -o test-data.json.gz
```

Mirrors `integration-load-tests generate` (same OPAQUE flow, same output
schema) but performs every round-trip over Kafka. The resulting envelope
additionally includes each client's final `state_jws`, which the load-test
command must seed back into the backend before running cycles.

### `load-test` — run sustained auth + sign cycles

```bash
loadtest-kafka load-test \
  --bootstrap-servers access-kafka-cluster-kafka-bootstrap:9092 \
  --server-pubkey-pem /path/to/server-pubkey.pem \
  --test-data test-data.json.gz \
  -t 32 --mean-delay-ms 50 -d 60
```

| flag | default | purpose |
| --- | --- | --- |
| `-t, --threads` | `32` | number of virtual users (Tokio tasks) |
| `--mean-delay-ms` | `0` | Poisson-shaped think time per VU (`0` = burst) |
| `-d, --duration-secs` | `60` | wall-clock test length (`0` = until Ctrl-C) |
| `--signs-per-cycle` | `1` | HSM sign ops per authenticated session |
| `--request-timeout-secs` | `30` | per-request response timeout |
| `--response-topic-partitions` | `16` | partitions for the per-process response topics |

`load-test` requires datasets produced by **`loadtest-kafka generate`** —
the REST-based generator can't capture `state_jws` (the BFF keeps it
private in Redis).

## Running in the k3s cluster

The cluster's Kafka cluster advertises internal hostnames only, so the
load-test must run as a Pod inside the cluster. The provided
`Containerfile` produces an image deployable as a `Job` — see the example
manifest in the repo's deployment docs.

```bash
# from wallet-r2ps/
docker build -t registry.dev.local:8443/diggsweden/loadtest-kafka:k3s \
  -f loadtest-kafka/Containerfile .
docker push registry.dev.local:8443/diggsweden/loadtest-kafka:k3s
```

## Known startup quirk

The very first cycle in each load-test run can fall into the consumer
group join window — the worker has already produced the response, but the
load-test's `StreamConsumer` is still completing its rebalance. With the
default 30 s request timeout each VU therefore logs **exactly one** auth
error during cold start, and successful cycles begin around `t≈30 s`. A
future improvement is to issue a warm-up request and block on it before
releasing the VUs.
