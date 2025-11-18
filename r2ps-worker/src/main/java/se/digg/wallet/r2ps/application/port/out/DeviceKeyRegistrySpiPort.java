package se.digg.wallet.r2ps.application.port.out;

import se.digg.wallet.r2ps.domain.aggregate.DeviceKey;

import java.util.Optional;
import java.util.UUID;

public interface DeviceKeyRegistrySpiPort {
  Optional<DeviceKey> getDeviceKey(UUID walletId, String kid);
}
