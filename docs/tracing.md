<!--
SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government

SPDX-License-Identifier: EUPL-1.2
-->

# OTLP tracing

`wallet-bff` and `hsm-worker` emit OTLP spans over gRPC and propagate
W3C tracecontext across the request path (HTTP → Kafka → HTTP). The
collector side is documented in `wallet-iac/docs/tracing.md`; this file
covers the **app-side wiring** and how to verify it locally.

## Trace tree

A single HTTP call to the BFF becomes one trace tree with three spans:

```text
[wallet-bff]  http_request                  ← root, from TraceLayer
 └─ [hsm-worker] process_request_kafka      ← child via Kafka traceparent header
     └─ [wallet-bff] consume_*_response     ← grand-child via response Kafka header
```

For the state-init topic (`POST /hsm/v1/device-states`) the operation
names are `process_state_init_kafka` and `consume_state_init_response`;
for the hsm-request topic (`POST /hsm/v1/requests`) they are
`process_request_kafka` and `consume_hsm_response`. Both topics close
the loop the same way.

If an Istio sidecar (or any upstream proxy) injects a `traceparent`
header on the incoming HTTP request, the BFF adopts it as the parent of
`http_request`, so the trace continues seamlessly from envoy.

## Wiring

Both services share the same shape:

| Concern | wallet-bff | hsm-worker |
|---|---|---|
| OTLP init | `wallet-bff/src/infrastructure/telemetry.rs` | `hsm-worker/src/infrastructure/telemetry.rs` |
| Propagator | `TraceContextPropagator` (W3C) registered globally | same |
| HTTP ingress | `TraceLayer` in `incoming/web/mod.rs` extracts `traceparent` from request headers, creates `http_request` span | n/a |
| Kafka producer | `tracecontext_headers()` in `outgoing/kafka/request_sender.rs` injects current span into outgoing message headers | `outgoing/r2ps_response_kafka_message_sender.rs`, `outgoing/state_init_response_kafka_sender.rs` |
| Kafka consumer | `incoming/kafka/r2ps_response_consumer.rs`, `incoming/kafka/state_init_response_consumer.rs` extract `traceparent` from message headers and `set_parent` on their span | `incoming/r2ps_request_kafka_message_receiver.rs`, `incoming/state_init_request_kafka_receiver.rs` |

Header injection / extraction is implemented once per service in
`infrastructure/kafka_propagation.rs` (`Injector` + `Extractor` over
`rdkafka::message::OwnedHeaders` / `BorrowedHeaders`).

## Configuration

| Env var | Default | Effect |
|---|---|---|
| `OTEL_EXPORTER_OTLP_ENDPOINT` | `http://gateway-collector.observability.svc:4317` | OTLP/gRPC endpoint. Default matches the cluster gateway. Override for local stacks. |
| `RUST_LOG` | per-service default (`wallet_bff=info,...` / `info`) | Controls the tracing-subscriber filter that feeds the OTLP layer. |

The exporter uses `with_tonic()` (gRPC, port 4317). Spans are batched
via `with_batch_exporter` on the Tokio runtime; the `TelemetryGuard`
returned from `telemetry::init` calls `shutdown_tracer_provider` on
Drop so in-flight spans flush before exit.

## Local verification

`docker-compose.otlp.yaml` is an overlay that swaps the services to the
dev release images and adds a `jaeger-all-in-one` container as the OTLP
sink. Use it to confirm the full trace tree end-to-end.

```bash
# Bring up the stack (use --no-build so the base compose's `build:`
# directives are ignored in favour of the overlay's `image:`).
docker compose -f docker-compose.yaml -f docker-compose.otlp.yaml up -d --no-build

# Drive traffic with the integration-load-tests image.
mkdir -p /tmp/iltdata
docker run --rm --network r2ps-dev_wallet-network \
  -v "$(pwd)/integration-load-tests/server-pubkey.pem:/server-pubkey.pem:ro" \
  -v /tmp/iltdata:/data \
  ghcr.io/diggsweden/wallet-r2ps/integration-load-tests:<dev-tag> \
  generate --bff-url http://wallet-bff-lb:8088 \
           --server-pubkey-pem /server-pubkey.pem \
           -n 3 -o /data/td.json.gz

docker run --rm --network r2ps-dev_wallet-network \
  -v "$(pwd)/integration-load-tests/server-pubkey.pem:/server-pubkey.pem:ro" \
  -v /tmp/iltdata:/data \
  ghcr.io/diggsweden/wallet-r2ps/integration-load-tests:<dev-tag> \
  load-test --bff-url http://wallet-bff-lb:8088 \
            --server-pubkey-pem /server-pubkey.pem \
            --test-data /data/td.json.gz -t 2 -d 5
```

Then check Jaeger:

- UI: <http://localhost:16686> — search service `wallet-bff`, expand any
  trace; expect 3 spans across `wallet-bff` and `hsm-worker`.
- API one-liner that confirms every recent trace spans both services:

  ```bash
  curl -s "http://localhost:16686/api/traces?service=wallet-bff&limit=20&lookback=1h" \
    | python3 -c 'import json,sys; d=json.load(sys.stdin); \
        print(sum(1 for t in d["data"] \
            if len({p["serviceName"] for p in t["processes"].values()}) > 1), \
            "/", len(d["data"]), "traces span both services")'
  ```

Tear down with `docker compose -f docker-compose.yaml -f docker-compose.otlp.yaml down -v`.

## Cluster deployment

When deployed via the gitops manifests, `OTEL_EXPORTER_OTLP_ENDPOINT` is
left at its default and the in-cluster Service
`gateway-collector.observability.svc:4317` resolves to the cluster's
OpenTelemetry Collector. From there the pipeline fans out to TempoStack
(traces). See `wallet-iac/docs/tracing.md` for the collector config,
exporters, and how to view traces in the OpenShift Console.
