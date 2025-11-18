package se.digg.wallet.r2ps.domain.event;

public record DeviceKeyRevoked(String deviceId, EventMetadata metadata) implements Event {
}
