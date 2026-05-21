<!--
SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government

SPDX-License-Identifier: EUPL-1.2
-->

# API Reference

Type specifications for the HSM Worker protocol, generated from the OpenAPI schema.

## Top-Level Messages

- [HsmWorkerRequestDto](hsm-worker-request-dto.md) — incoming Kafka request envelope
- [WorkerResponseJws](worker-response-jws.md) — outgoing Kafka response envelope

## Protocol Envelopes

- [OuterRequest](outer-request.md) / [OuterResponse](outer-response.md) — signed outer layer
- [InnerRequest](inner-request.md) / [InnerResponse](inner-response.md) — encrypted inner layer
- [TypedJws\<T\>](typed-jws-wrapper.md) — type-safe JWS wrapper
- [TypedJwe\<T\>](typed-jwe-wrapper.md) — type-safe JWE wrapper

## State

- [DeviceHsmState](device-hsm-state.md) — persisted device state

## Identifiers and Status

- [SessionId](session-id.md)
- [OperationId](operation-id.md)
- [Status](status.md)
