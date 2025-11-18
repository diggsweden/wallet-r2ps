package se.digg.wallet.r2ps.domain.aggregate;

import io.soabase.recordbuilder.core.RecordBuilder;

import java.time.Instant;
import java.util.List;
import java.util.Optional;
import java.util.UUID;

@RecordBuilder
public record ServerWallet(UUID walletId, List<HsmKey> hsmKeys, List<DeviceKey> deviceKeys, Instant revoked, Instant created, Instant updated, int version) {

  public Optional<HsmKey> hsmKeyByKeyId(String keyId) {
    return hsmKeys().stream().filter(k -> k != null && k.keyId() != null && k.keyId().equals(keyId)).findFirst();
  }

  public List<HsmKey> hsmKeysByCurve(String curve) {
    return hsmKeys().stream().filter(k -> k != null && k.keyId() != null && k.curveName().equals(curve)).toList();
  }

  public Optional<DeviceKey> device(String deviceId) {
    return deviceKeys().stream().filter(d -> d != null && d.deviceId() != null && d.deviceId().equals(deviceId)).findFirst();
  }

}
