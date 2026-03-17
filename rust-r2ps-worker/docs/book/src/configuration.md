# Configuration

All configuration is loaded from environment variables (with `.env` file support via `dotenvy`).

## CLI commands

```
hsm-worker [COMMAND]

Commands:
  run                   Run the worker (default)
  bootstrap-snapshot    Bootstrap the state-snapshot Kafka topic from PostgreSQL
    --device-id <ID>    Optional: filter to a specific device_id
```

- **`run`** (default): Starts all three threads and blocks until shutdown.
- **`bootstrap-snapshot`**: One-shot utility that reads device states from PostgreSQL and publishes them to the `state-snapshot` Kafka topic. Used to rebuild the snapshot topic from the database.

## Environment variables

### Cryptographic keys

| Variable | Required | Default | Description |
|----------|:---:|---------|-------------|
| `SERVER_PRIVATE_KEY` | Yes | -- | Base64-encoded PEM EC private key for JWS signing |
| `SERVER_PUBLIC_KEY` | Yes | -- | Base64-encoded PEM EC public key for JWS verification |

### OPAQUE protocol

| Variable | Required | Default | Description |
|----------|:---:|---------|-------------|
| `OPAQUE_SERVER_SETUP` | No | -- | OPAQUE server setup bytes (base64). Generated on first run if not set. |
| `OPAQUE_SERVER_IDENTIFIER` | No | `cloud-wallet.digg.se` | OPAQUE protocol server identifier |
| `OPAQUE_CONTEXT` | No | `RPS-Ops` | OPAQUE protocol context string |

### PKCS#11 / HSM

| Variable | Required | Default | Description |
|----------|:---:|---------|-------------|
| `PKCS11_LIB` | Yes | -- | Path to the PKCS#11 shared library (e.g. `/usr/lib/softhsm/libsofthsm2.so`) |
| `PKCS11_SLOT_TOKEN_LABEL` | Yes | -- | HSM token label |
| `PKCS11_SO_PIN` | No | -- | HSM Security Officer PIN |
| `PKCS11_USER_PIN` | No | -- | HSM User PIN |
| `PKCS11_WRAP_KEY_ALIAS` | Yes | -- | AES wrap key alias for key wrapping |

### Kafka

| Variable | Required | Default | Description |
|----------|:---:|---------|-------------|
| `KAFKA_BOOTSTRAP_SERVERS` | Yes | -- | Kafka broker addresses |
| `KAFKA_BROKER_ADDRESS_FAMILY` | No | `v4` | `v4` or `v6` |
| `KAFKA_GROUP_ID` | No | `rust-grp` | Consumer group ID |
| `KAFKA_GROUP_INSTANCE_ID` | No | `consumer-1` | Static group membership instance ID |

### PostgreSQL

| Variable | Required | Default | Description |
|----------|:---:|---------|-------------|
| `POSTGRES_HOST` | No | `localhost` | PostgreSQL host |
| `POSTGRES_PORT` | No | `5432` | PostgreSQL port |
| `POSTGRES_DB` | No | `r2ps` | Database name |
| `POSTGRES_USER` | No | `r2ps` | Database user |
| `POSTGRES_PASSWORD` | No | `secret` | Database password |

### Worker tuning

| Variable | Required | Default | Description |
|----------|:---:|---------|-------------|
| `STATE_CACHE_PATH` | No | `/tmp/tamper-cache.redb` | Path to the redb tamper detection cache file |
| `STATE_CACHE_CAPACITY` | No | `1000000` | Maximum entries in the Moka in-memory cache |
| `CATCHUP_WORKERS` | No | `max(1, num_cpus - 2)` | Parallel workers for snapshot catch-up phase |
| `POD_ID` | No | `$HOSTNAME` | Pod/instance identifier for logging |
| `RUST_LOG` | No | `info` | Tracing filter level (standard `EnvFilter` syntax) |

## Database schema

The worker expects three tables in PostgreSQL, created by `config/init-db.sql`:

```sql
-- Aggregate head for optimistic concurrency control
CREATE TABLE device_state_head (
    device_id       TEXT        PRIMARY KEY,
    current_version BIGINT      NOT NULL,
    updated_at      TIMESTAMPTZ DEFAULT now()
);

-- Append-only state version log
CREATE TABLE device_state_version (
    device_id       TEXT        NOT NULL,
    version         BIGINT      NOT NULL,
    state_jws       TEXT        NOT NULL,
    command_type    TEXT        NOT NULL,
    correlation_id  TEXT        NOT NULL,
    created_at      TIMESTAMPTZ DEFAULT now(),
    PRIMARY KEY (device_id, version)
);

-- Transactional outbox for reliable Kafka publishing
CREATE TABLE outbox (
    id          BIGSERIAL   PRIMARY KEY,
    topic       TEXT        NOT NULL,
    key         TEXT        NOT NULL,
    payload     JSONB       NOT NULL,
    created_at  TIMESTAMPTZ DEFAULT now(),
    published   BOOLEAN     DEFAULT false
);
```
