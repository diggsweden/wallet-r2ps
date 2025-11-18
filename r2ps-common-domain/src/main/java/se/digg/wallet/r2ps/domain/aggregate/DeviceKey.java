package se.digg.wallet.r2ps.domain.aggregate;

import io.soabase.recordbuilder.core.RecordBuilder;

import java.io.IOException;
import java.security.KeyFactory;
import java.security.PublicKey;
import java.security.spec.InvalidKeySpecException;
import java.security.spec.X509EncodedKeySpec;
import java.time.Instant;
import java.util.Base64;
import java.util.Optional;
import java.util.UUID;

@RecordBuilder
public record DeviceKey(UUID walletId, String deviceId, String devicePublicKeyBase64, Optional<Instant> revoked, Instant created, Instant updated) {

  public DeviceKey(UUID walletId, String deviceId, PublicKey devicePublicKey, Optional<Instant> revoked, Instant created, Instant updated) {
    this(walletId, deviceId, Base64.getEncoder().encodeToString(devicePublicKey.getEncoded()), revoked, created, updated);
  }

  public String kid() {
    return deviceId;
  }

  public PublicKey devicePublicKey() {
    try {
      byte[] keyBytes = Base64.getDecoder().decode(devicePublicKeyBase64);
      X509EncodedKeySpec keySpec = new X509EncodedKeySpec(keyBytes);

      // Try RSA first
      try {
        return KeyFactory.getInstance("RSA").generatePublic(keySpec);
      } catch (InvalidKeySpecException e) {
        // If RSA fails, try EC
        return KeyFactory.getInstance("EC").generatePublic(keySpec);
      }
    } catch (Exception e) {
      throw new RuntimeException("Failed to deserialize public key", e);
    }
  }

  public DeviceKey devicePublicKey(PublicKey devicePublicKey) {
    return DeviceKeyBuilder.builder(this).devicePublicKeyBase64(Base64.getEncoder().encodeToString(devicePublicKey.getEncoded())).build();
  }
}
