package se.digg.wallet.r2ps.domain.event;

import java.security.PublicKey;
import java.time.Instant;

// TODO check correct attributes
public record HsmKeyCreated(String kid, String curveName, Instant creationTime, PublicKey publicKey, EventMetadata metadata) implements Event {
}
