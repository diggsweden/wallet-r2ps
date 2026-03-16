# R2PS REST API (Rust)

A Rust implementation of the Remote to Physical Signing (R2PS) REST API, replacing the Java Spring Boot version with a high-performance, memory-safe alternative.

**✅ Fully compliant with Swedish REST API Profile v1.2.0 (dataportal.se)**

This implementation follows **REST Maturity Level 3 (HATEOAS)** according to Richardson's REST Maturity Model and meets all MUST (SKALL) requirements of the Swedish public sector REST API profile published by DIGG.

## Features

- ✅ **REST Level 3 (HATEOAS)**: All responses include hypermedia links for discoverability
- ✅ **Async Request/Response**: Support for both synchronous and asynchronous processing
- ✅ **OpenAPI/Swagger**: Auto-generated API documentation
- ✅ **Redis Integration**: State management with TTL-based expiration
- ✅ **Kafka Integration**: Event-driven architecture for HSM worker communication
- ✅ **Type Safety**: Strong typing with Rust's type system
- ✅ **Performance**: High throughput with Tokio async runtime
- ✅ **Observability**: Structured logging with tracing

## Architecture

The application follows **Hexagonal Architecture** (Ports and Adapters):

```
┌─────────────────────────────────────────────────────────┐
│                    INFRASTRUCTURE                        │
│  ┌──────────────────────────────────────────────────┐  │
│  │  HTTP Handlers (Axum)                            │  │
│  │  - POST /                                        │  │
│  │  - GET /task/{id}                                │  │
│  │  - POST /new_state                               │  │
│  └──────────────────┬───────────────────────────────┘  │
│                     │                                    │
│  ┌──────────────────▼───────────────────────────────┐  │
│  │         APPLICATION LAYER                        │  │
│  │  - R2psService (Business Logic)                  │  │
│  └────────────────┬──────────────────────────────────┘  │
│                   │                                      │
│  ┌────────────────▼──────────────────────────────────┐  │
│  │  ADAPTERS (OUT)                                   │  │
│  │  - RedisRepository (State)                        │  │
│  │  - KafkaProducer (Messaging)                      │  │
│  │  - KafkaConsumer (Messaging)                      │  │
│  └───────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

## API Endpoints

All endpoints are versioned and follow Swedish REST API Profile naming conventions (plural nouns, lowercase with hyphens).

**Base URL**: `https://api.digg.se/r2ps-api/v1` (production)  
**Dev URL**: `https://localhost:8088/r2ps-api/v1`

### Main Endpoints

#### `POST /r2ps-api/v1/`
Submit a service request to the HSM worker.

**Request:**
```json
{
  "clientId": "uuid-string",
  "outerRequestJws": "jws-token"
}
```

**Response (202 Accepted - Async):**
```json
{
  "correlationId": "uuid",
  "status": "PENDING",
  "resultUrl": "/r2ps-api/v1/requests/{uuid}",
  "_links": {
    "self": {
      "href": "https://api.digg.se/r2ps-api/v1/requests/{uuid}",
      "method": "GET",
      "rel": "self"
    },
    "poll": {
      "href": "https://api.digg.se/r2ps-api/v1/requests/{uuid}",
      "method": "GET",
      "rel": "poll"
    }
  }
}
```

#### `GET /r2ps-api/v1/requests/{correlationId}`
Poll for the result of an async request (renamed from `/task/{id}` for compliance).

**Response (200 OK - Complete):**
```json
{
  "correlationId": "uuid",
  "status": "COMPLETE",
  "result": "jws-response-token",
  "_links": {
    "self": {
      "href": "https://api.digg.se/r2ps-api/v1/requests/{uuid}",
      "method": "GET"
    }
  }
}
```

#### `POST /r2ps-api/v1/device-states`
Initialize a new device with a public key (renamed from `/new_state` for compliance).

**Request:**
```json
{
  "publicKey": {
    "kty": "EC",
    "crv": "P-256",
    "x": "base64url",
    "y": "base64url"
  },
  "ttl": "P30D"
}
```

**Response (200 OK):**
```json
{
  "status": "OK",
  "clientId": "uuid",
  "devAuthorizationCode": "auth-code",
  "_links": {
    "self": {
      "href": "https://api.digg.se/r2ps-api/v1/device-states",
      "method": "GET"
    },
    "submit-request": {
      "href": "https://api.digg.se/r2ps-api/v1/",
      "method": "POST",
      "rel": "submit-request",
      "type": "application/json"
    }
  }
}
```

#### `GET /r2ps-api/v1/api-info`
API metadata endpoint (required by Swedish REST API Profile VER.06).

**Response (200 OK):**
```json
{
  "apiName": "r2ps-api",
  "apiVersion": "0.1.0",
  "apiReleased": "2024-01-01",
  "apiDocumentation": "https://api.digg.se/r2ps-api/v1/",
  "apiStatus": "alpha"
}
```

### Additional Endpoints

- `GET /r2ps-api/v1/health` - Health check
- `GET /r2ps-api/v1/openapi.json` - OpenAPI 3.0 specification

### API Documentation

- **OpenAPI Specification**: `https://localhost:8088/r2ps-api/v1/openapi.json` (complies with DOK.23, DOK.24)
- **Swagger UI**: `https://localhost:8088/swagger-ui`
- **API Info**: `https://localhost:8088/r2ps-api/v1/api-info` (required by VER.06)

## HATEOAS Implementation

All responses include `_links` objects that provide:

- **Discoverability**: Clients can navigate the API through links
- **Decoupling**: Clients don't need to construct URLs
- **Evolution**: API URLs can change without breaking clients

Example link structure (HYP.11-HYP.17):
```json
{
  "_links": {
    "self": {
      "href": "https://api.digg.se/r2ps-api/v1/requests/123e4567-e89b-12d3-a456-426614174000",
      "method": "GET",
      "rel": "self"
    },
    "poll": {
      "href": "https://api.digg.se/r2ps-api/v1/requests/123e4567-e89b-12d3-a456-426614174000",
      "method": "GET",
      "rel": "poll"
    }
  }
}
```

Note: Links use **absolute URLs** and **always include method** as required by the Swedish profile.

---

## 🇸🇪 Swedish REST API Profile Compliance

This API is **fully compliant** with the Swedish REST API Profile v1.2.0 (dataportal.se/rest-api-profil).

### ✅ MUST Requirements (SKALL)

| Requirement | Status | Implementation |
|-------------|--------|----------------|
| **MOG.01** - Level 2 (resources + HTTP verbs) | ✅ | Multiple resources, correct HTTP methods |
| **MOG.02** - Level 3 (HATEOAS) recommended | ✅ | All responses include `_links` |
| **UFN.02** - HTTPS on port 443 | ✅ | HTTPS enforced, configurable (SAK.01-03) |
| **RES.06** - Plural noun resources | ✅ | `/requests`, `/device-states` |
| **VER.05** - Major version in URL | ✅ | `/v1/` in all endpoints |
| **VER.06-07** - `/api-info` endpoint | ✅ | `GET /r2ps-api/v1/api-info` |
| **DOK.16** - Machine-readable spec | ✅ | OpenAPI 3.0 at `/v1/openapi.json` |
| **DOK.23-24** - Spec at versioned path | ✅ | `/r2ps-api/v1/openapi.json` |
| **DOT.01-02** - RFC 3339 dates | ✅ | ISO 8601/RFC 3339 format used |
| **FEL.01-02** - RFC 9457 error format | ✅ | `application/problem+json` |
| **HYP.05** - Every response has `self` link | ✅ | All responses include self link |
| **HYP.12** - Links use absolute URLs | ✅ | Full URLs with scheme + domain |
| **HYP.17** - Method always present in links | ✅ | Non-optional `method` field |
| **SAK.01-03** - HTTPS/TLS 1.2+ | ✅ | Enforced via middleware |
| **SAK.17** - CORS only when necessary | ✅ | Configurable, no wildcard `*` |
| **SAK.25-26** - No internal details in errors | ✅ | Generic error messages |
| **AME.01-02** - JSON default | ✅ | `application/json` |
| **AME.04-05** - Consistent field naming | ✅ | camelCase throughout |

### 🛡️ Security Features

- **TLS 1.2+ enforced** (SAK.01-SAK.03)
- **HTTPS-only** - HTTP requests rejected, not redirected
- **No wildcard CORS** - Specific origins only (SAK.17)
- **JWS request signing** - All requests verified by HSM worker
- **No internal details in errors** - RFC 9457 compliant (SAK.25-26)
- **UUID identifiers** - Non-sequential, globally unique (RES.01-03)

### 📝 Error Response Example (RFC 9457)

When errors occur, the API returns RFC 9457 Problem Details:

```json
{
  "type": "https://api.digg.se/r2ps/v1/problems/device-not-found",
  "title": "Device Not Found",
  "status": 404,
  "detail": "Device with ID 'abc123' was not found",
  "instance": "/r2ps-api/v1/device-states/abc123"
}
```

Response header: `Content-Type: application/problem+json`

## Configuration

Configuration is done via environment variables or a `.env` file:

```env
# Server
SERVER__PORT=8088
SERVER__HOST=0.0.0.0
SERVER__CONTEXT_PATH=/r2ps-api/v1
SERVER__BASE_URL=https://localhost:8088/r2ps-api/v1

# Security (Swedish REST API Profile compliance)
SERVER__REQUIRE_HTTPS=true              # Enforce HTTPS (SAK.01-03) - set to false only for local dev
SERVER__CORS_ALLOWED_ORIGINS=           # Comma-separated origins (SAK.17) - leave empty to disable CORS

# Redis
REDIS__HOST=localhost
REDIS__PORT=6379

# Kafka
KAFKA__BROKERS=localhost:9092
KAFKA__CONSUMER__GROUP_ID=r2ps-rest-api-group

# R2PS
R2PS__SERVE_SYNC=true
R2PS__SYNC_TIMEOUT_MS=3000
R2PS__RESPONSE_TTL_SECONDS=600
R2PS__DEVICE_STATE_TTL_SECONDS=2592000
```

### Security Configuration

**Production settings** (Swedish REST API Profile compliant):
```env
SERVER__BASE_URL=https://api.digg.se/r2ps-api/v1
SERVER__REQUIRE_HTTPS=true
SERVER__CORS_ALLOWED_ORIGINS=https://app.digg.se
```

**Development settings** (local only):
```env
SERVER__BASE_URL=https://localhost:8088/r2ps-api/v1
SERVER__REQUIRE_HTTPS=false
SERVER__CORS_ALLOWED_ORIGINS=
```

See `.env.example` for all available options.

## Building

### Prerequisites

- **Rust 1.85 or later** (Rust 2024 edition)
- Redis 6.0 or later
- Kafka 2.8 or later

### Build

```bash
cargo build --release
```

### Run

```bash
# Copy example env file
cp .env.example .env

# Edit .env with your configuration
vim .env

# Run the application
cargo run --release
```

### Docker

```bash
# Build image
docker build -f Containerfile -t r2ps-rest-api:latest .

# Run container
docker run -p 8088:8088 --env-file .env r2ps-rest-api:latest
```

## Development

### Project Structure

```
src/
├── config.rs           # Configuration management
├── error.rs            # Error types and handling
├── handlers/           # HTTP request handlers
│   └── r2ps_handlers.rs
├── lib.rs              # Library root
├── main.rs             # Application entry point
├── messaging/          # Kafka integration
│   ├── handlers.rs     # Kafka message handlers
│   └── kafka_client.rs # Kafka producer
├── models/             # Domain models
│   ├── dto.rs          # Data transfer objects
│   └── hateoas.rs      # HATEOAS link structures
├── repositories/       # Data access
│   └── redis_repository.rs
└── services/           # Business logic
    └── r2ps_service.rs
```

### Testing

```bash
# Run tests
cargo test

# Run with logging
RUST_LOG=debug cargo test
```

## Comparison with Java Version

| Feature | Java (Spring Boot) | Rust (This) |
|---------|-------------------|-------------|
| Runtime | JVM | Native binary |
| Memory Safety | Garbage collected | Compile-time guaranteed |
| Async I/O | WebFlux | Tokio |
| Startup Time | ~2-3 seconds | ~50ms |
| Memory Usage | ~200-500MB | ~20-50MB |
| Binary Size | JAR ~50MB | ~10MB |
| HTTP Server | Netty | Axum/Hyper |
| JSON Parsing | Jackson | Serde |

## REST Level 3 Compliance

This API follows REST Maturity Level 3 (HATEOAS) as required by the Swedish REST API Profile:

- ✅ **Level 0** (MOG.01): HTTP as transport mechanism
- ✅ **Level 1** (MOG.01): Individual resources with unique URIs (`/requests/{id}`, `/device-states`)
- ✅ **Level 2** (MOG.01): HTTP methods (GET, POST) and status codes (200, 202, 404, etc.)
- ✅ **Level 3** (MOG.02): Hypermedia controls (HATEOAS) with `_links` in all responses

### API Versioning Strategy (VER.04, VER.05)

- **Current version**: `v1` (0.1.0) - Alpha status since MAJOR = 0 (VER.11)
- **Semantic versioning**: MAJOR.MINOR.PATCH
  - **MAJOR**: Breaking changes (requires new `/v2/` path)
  - **MINOR**: New features, backward-compatible
  - **PATCH**: Bug fixes, backward-compatible
- **Version in URL**: Only MAJOR version appears in path (`/v1/`)
- **Full version**: Available via `/api-info` endpoint

## Security Considerations

- **JWS Verification**: All requests are JWS-signed (validated by HSM worker)
- **State Integrity**: Device state stored as JWS (tamper-evident)
- **TTL-Based Expiration**: Device states expire automatically
- **Correlation Tracking**: Request IDs prevent response confusion

## License

Copyright © 2024 DIGG - Swedish Agency for Digital Government

## Support

For questions or issues, contact: info@digg.se
