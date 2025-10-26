package se.digg.wallet.r2ps.domain.event;

import java.security.PublicKey;

public record DeviceKeyAdded(String deviceId, PublicKey devicePublicKey, EventMetadata metadata) implements Event{
}
