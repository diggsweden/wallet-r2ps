# integration-load-tests

Integration and load testing tool for the R2PS wallet system. Implements a native Rust client with full OPAQUE protocol support, PIN stretching, and JWS/JWE envelope handling — matching the Android/Swift access mechanism libraries.

## Prerequisites

- Rust toolchain (1.70+)
- A running R2PS environment (BFF + HSM worker + Kafka + Redis/Valkey + PostgreSQL)
  - Start with `make up` from the `wallet-r2ps/` root
- The server's EC P-256 public key in PEM format (from `.env.opaque`)

## Build

```bash
cargo build --release
```

The binary is at `target/release/integration-load-tests`.

## Commands

### `generate` — Create test data

Registers N clients through the BFF REST API, performing the full onboarding flow for each:

1. Generate EC P-256 device keypair + PIN stretch keypair
2. Initialize device state (`POST /r2ps-api/new_state`)
3. OPAQUE PIN registration (start + finish)
4. OPAQUE login / create session (start + finish)
5. Generate an HSM key

The output is a gzip-compressed JSON file containing all client credentials needed for load testing.

```bash
cargo run --release -- generate \
  --bff-url http://localhost:8088 \
  --server-pubkey-pem path/to/server-pubkey.pem \
  -n 100 \
  --pin 123456 \
  -o test-data.json.gz \
  -c 8
```

#### Options

| Option | Default | Description |
|--------|---------|-------------|
| `--bff-url` | *(required)* | BFF base URL |
| `--server-pubkey-pem` | *(required)* | Path to server EC P-256 public key PEM |
| `-n, --count` | `10` | Number of clients to generate |
| `--pin` | `123456` | PIN to use for all clients |
| `-o, --output` | `test-data.json.gz` | Output file path (gzip JSON) |
| `-c, --concurrency` | `4` | Parallel registrations |
| `--opaque-context` | `RPS-Ops` | OPAQUE protocol context string |
| `--opaque-server-id` | `dev.cloud-wallet.digg.se` | OPAQUE server identifier |
| `--ttl` | `P30D` | Device state TTL (ISO 8601 duration) |

#### Output format

```json
{
  "opaque_context": "RPS-Ops",
  "opaque_server_identifier": "dev.cloud-wallet.digg.se",
  "clients": [
    {
      "client_id": "uuid",
      "kid": "base64url-jwk-thumbprint",
      "pin": "123456",
      "pin_stretch_d": "base64url-ec-private-scalar",
      "device_key": {
        "kty": "EC", "crv": "P-256",
        "x": "...", "y": "...", "d": "...", "kid": "..."
      },
      "hsm_kid": "uuid"
    }
  ]
}
```

### `load-test` — Run load tests

Runs concurrent worker tasks that each repeatedly:

1. Pick a random client from the test data
2. Create an OPAQUE session (authenticate_start + authenticate_finish)
3. Perform N HSM sign operations
4. Repeat with a new random client

```bash
cargo run --release -- load-test \
  --bff-url http://localhost:8088 \
  --server-pubkey-pem path/to/server-pubkey.pem \
  --test-data test-data.json.gz \
  -t 8 \
  --mean-delay-ms 100 \
  -d 60 \
  --signs-per-cycle 5
```

#### Options

| Option | Default | Description |
|--------|---------|-------------|
| `--bff-url` | *(required)* | BFF base URL |
| `--server-pubkey-pem` | *(required)* | Path to server EC P-256 public key PEM |
| `--test-data` | *(required)* | Path to test data file (`.json.gz` or `.json`) |
| `-t, --threads` | `4` | Number of concurrent worker tasks |
| `--mean-delay-ms` | `100` | Mean inter-request delay per worker in ms (0 = burst) |
| `-d, --duration-secs` | `60` | Test duration in seconds (0 = unlimited) |
| `--signs-per-cycle` | `5` | HSM sign operations per authentication cycle |
| `--stats-interval-secs` | `5` | How often to print stats summary |

#### Traffic shaping

Inter-request delays follow an exponential distribution with the configured mean, producing Poisson-distributed arrivals across workers. Delays are clamped to 5x the mean to prevent extreme outliers.

Set `--mean-delay-ms 0` for burst mode (no delays, maximum throughput).

#### Stats output

During the test, a summary line is printed every `--stats-interval-secs`:

```
[5s] reqs=142 err=0 auth=28 auth_err=0 rps=28.40 avg=34ms p50=31ms p95=62ms p99=78ms max=91ms
```

At completion, a full report is printed:

```
--- Load Test Report ---
Duration:      60s
Total reqs:    1847
Total errors:  0
Auth cycles:   264
Auth errors:   0
Throughput:    30.78 req/s
Avg latency:   33ms
p50 latency:   30ms
p95 latency:   61ms
p99 latency:   82ms
Max latency:   134ms
------------------------
```

## Examples

### Quick smoke test (5 clients, 10 second load test)

```bash
# Generate 5 test clients
cargo run --release -- generate \
  --bff-url http://localhost:8088 \
  --server-pubkey-pem server-pubkey.pem \
  -n 5 -o smoke-test.json.gz

# Run a short load test
cargo run --release -- load-test \
  --bff-url http://localhost:8088 \
  --server-pubkey-pem server-pubkey.pem \
  --test-data smoke-test.json.gz \
  -t 2 -d 10 --signs-per-cycle 3
```

### High-throughput burst test

```bash
# Generate 500 clients
cargo run --release -- generate \
  --bff-url http://localhost:8088 \
  --server-pubkey-pem server-pubkey.pem \
  -n 500 -c 16 -o large-dataset.json.gz

# Burst mode, 32 workers, 2 minute duration
cargo run --release -- load-test \
  --bff-url http://localhost:8088 \
  --server-pubkey-pem server-pubkey.pem \
  --test-data large-dataset.json.gz \
  -t 32 --mean-delay-ms 0 -d 120 --signs-per-cycle 10
```

### Steady-state rate-limited test

```bash
cargo run --release -- load-test \
  --bff-url http://localhost:8088 \
  --server-pubkey-pem server-pubkey.pem \
  --test-data test-data.json.gz \
  -t 8 --mean-delay-ms 200 -d 300
```

## Verbose logging

Set the `RUST_LOG` environment variable for debug output:

```bash
RUST_LOG=debug cargo run --release -- generate ...
RUST_LOG=integration_load_tests=debug cargo run --release -- load-test ...
```

## Architecture

```
src/
├── crypto/              # Ported from opaque-ke-wasm — same cipher suite as all platforms
│   ├── pin_stretch.rs   # hash-to-curve(P-256, SHA-256) → ECDH → HKDF-SHA256 → 32 bytes
│   ├── opaque_client.rs # OPAQUE registration & login (P-256, TripleDh, Identity KSF)
│   └── keygen.rs        # EC P-256 keypair generation + JWK thumbprint (RFC 7638)
├── protocol/            # Ported from android-access-mechanism / wallet-test-bff-ws
│   ├── types.rs         # Wire types matching the BFF/worker JSON format
│   ├── message_builder.rs  # JWS(ES256) → JWE(ECDH-ES or dir + A256GCM) envelope builder
│   └── response_parser.rs  # JWS decode → JWE decrypt → InnerResponse parser
├── client/
│   ├── rest_client.rs      # HTTP client for BFF REST API (with 404 retry for race conditions)
│   └── access_mechanism.rs # High-level client matching Android OpaqueClient API
├── model/
│   └── test_data.rs     # Test data envelope — gzip JSON serialization
└── commands/
    ├── generate.rs      # Concurrent client registration with semaphore-bounded parallelism
    └── load_test.rs     # Multi-task load test with Poisson traffic shaping
```
