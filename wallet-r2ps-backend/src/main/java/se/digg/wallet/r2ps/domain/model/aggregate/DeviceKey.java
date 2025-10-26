package se.digg.wallet.r2ps.domain.model.aggregate;

import io.soabase.recordbuilder.core.RecordBuilder;

import java.security.PublicKey;
import java.time.Instant;
import java.util.Optional;
import java.util.UUID;

@RecordBuilder
public record DeviceKey(UUID walletId, String deviceId, PublicKey devicePublicKey, Optional<Instant> revoked, Instant created, Instant updated) {

}
