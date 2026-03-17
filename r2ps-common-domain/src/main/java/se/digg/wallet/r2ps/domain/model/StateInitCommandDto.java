package se.digg.wallet.r2ps.domain.model;

/**
 * Command DTO for device state initialization, sent to the r2ps-requests topic.
 * The worker creates a version-0 DeviceHsmState with the provided public key.
 *
 * @param correlationId server-generated UUID for request correlation
 * @param deviceId      device identifier
 * @param context       command context, always "state-init"
 * @param publicKey     EC P-256 public key of the device
 */
public record StateInitCommandDto(
    String correlationId,
    String deviceId,
    String context,
    EcPublicJwk publicKey) {}
