# Introduction

The HSM Worker is a multi-threaded Rust service that processes cryptographic operations for wallet devices. It manages device state, OPAQUE-based PIN authentication, and HSM-backed key operations (generation, signing, deletion).

## What it does

- **Device lifecycle**: Initialize device state, register and change PINs via OPAQUE PAKE
- **Authentication**: PIN-based authentication using the OPAQUE protocol, which derives a shared session key without ever transmitting the PIN
- **HSM key management**: Generate, list, delete EC keys on a PKCS#11 HSM (SoftHSM in dev, hardware HSM in production)
- **ECDSA signing**: Sign payloads using HSM-held private keys

## How it communicates

The worker is event-driven. It consumes commands from Kafka, processes them against server-owned device state in PostgreSQL, and publishes responses back through Kafka via a transactional outbox.

All request and response payloads use a two-layer JWS/JWE envelope for integrity and confidentiality. The outer layer is JWS-signed for authentication; the inner layer is JWE-encrypted using either a session key (for authenticated operations) or the device's public key (for unauthenticated operations like state initialization).

## Documentation structure

- **[Architecture](architecture/overview.md)** -- Hexagonal design, request pipeline, state management, security model, and threading
- **[Operations](operations/README.md)** -- All supported operations and their characteristics
- **[Configuration](configuration.md)** -- Environment variables and CLI commands
- **[API Reference](api-reference/README.md)** -- Detailed type specifications for the protocol
