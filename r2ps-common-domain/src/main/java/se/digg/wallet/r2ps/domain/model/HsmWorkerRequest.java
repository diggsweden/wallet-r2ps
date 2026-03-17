package se.digg.wallet.r2ps.domain.model;

/**
 * Command sent to the HSM worker via the r2ps-requests Kafka topic.
 * State is now server-owned in PostgreSQL — no stateJws field.
 *
 * @param correlationId server-generated UUID for request correlation
 * @param deviceId      device identifier (used as Kafka key for partition affinity)
 * @param requestId     client-generated request ID (WebSocket clients only), nullable
 * @param stateVersion  optimistic concurrency version, nullable
 * @param outerRequestJws JWS-encoded outer request envelope
 */
public record HsmWorkerRequest(
    String correlationId,
    String deviceId,
    String requestId,
    Long stateVersion,
    String outerRequestJws) {}
