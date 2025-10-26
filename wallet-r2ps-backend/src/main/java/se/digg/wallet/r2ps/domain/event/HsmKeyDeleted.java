package se.digg.wallet.r2ps.domain.event;

public record HsmKeyDeleted(String keyId, EventMetadata metadata) implements Event {
}
