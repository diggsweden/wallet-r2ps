package se.digg.wallet.r2ps.domain.model;

import java.util.Optional;

/**
 * Response from the HSM worker received via the r2ps-responses Kafka topic.
 * State is now server-owned — no stateJws field.
 *
 * @param correlationId    correlation ID matching the original request
 * @param deviceId         device identifier for Kafka partition key affinity
 * @param requestId        client-generated request ID (WebSocket clients only), nullable
 * @param outerResponseJws JWS-encoded service response, empty on error
 * @param status           "OK" or "ERROR"
 * @param errorMessage     error description when status != "OK", nullable
 */
public record R2psResponse(
    String correlationId,
    String deviceId,
    String requestId,
    Optional<String> outerResponseJws,
    String status,
    String errorMessage) {}
