package se.digg.wallet.r2ps.domain.event;

public record SignLogged(String deviceId, String keyId, EventMetadata metadata) implements Event {
}
