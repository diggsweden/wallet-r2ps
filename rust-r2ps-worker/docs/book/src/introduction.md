# Introduction

This documentation describes the domain model for the R2PS (Remote to Phone Signing) HSM Worker service.

## Purpose

The HSM Worker processes cryptographic operations for remote signing, managing device state and HSM-backed keys. This service communicates via Kafka, receiving requests and sending responses in a structured format.

## Documentation Structure

- **API Reference**: Detailed specifications of all data types used in the protocol
  - Request/Response DTOs
  - Protocol envelopes
  - State management types
  - Supporting types and enums

## Key Concepts

- **JWS (JSON Web Signature)**: Signed payloads ensuring integrity and authenticity
- **JWE (JSON Web Encryption)**: Encrypted payloads ensuring confidentiality
- **Device State**: Persistent state encoded as JWS, containing keys and metadata
- **Outer/Inner Layers**: Protocol uses nested envelopes for signed and encrypted data

For detailed type specifications, see the [API Reference](api-reference/index.md).
