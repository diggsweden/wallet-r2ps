package se.digg.wallet.r2ps.domain.aggregate;

import io.soabase.recordbuilder.core.RecordBuilder;

import java.security.PublicKey;
import java.time.Instant;
import java.util.UUID;

@RecordBuilder
public record HsmKey(
    UUID walletId,
    String keyId,
    String curveName,
    PublicKey publicKey,
    Instant created
) {

}
