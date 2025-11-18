package se.digg.wallet.r2ps.domain.event;

public record JwsSignLogged(String walletId, String deviceId, String keyId, EventMetadata metadata) implements
    Event {
}
