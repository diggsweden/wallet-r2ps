package se.digg.wallet.r2ps.application.service.aggregate;

import org.springframework.stereotype.Service;
import se.digg.wallet.r2ps.domain.event.DeviceKeyRevoked;
import se.digg.wallet.r2ps.domain.event.EventMetadata;
import se.digg.wallet.r2ps.domain.event.HsmKeyCreated;
import se.digg.wallet.r2ps.domain.event.HsmKeyDeleted;
import se.digg.wallet.r2ps.domain.event.ServerWalletRevoked;
import se.digg.wallet.r2ps.domain.exception.VersionConflict;
import se.digg.wallet.r2ps.domain.exception.WalletNotFoundException;
import se.digg.wallet.r2ps.domain.model.aggregate.DeviceKey;
import se.digg.wallet.r2ps.domain.model.aggregate.DeviceKeyBuilder;
import se.digg.wallet.r2ps.domain.model.aggregate.HsmKey;
import se.digg.wallet.r2ps.domain.model.aggregate.HsmKeyBuilder;
import se.digg.wallet.r2ps.domain.model.aggregate.ServerWallet;
import se.digg.wallet.r2ps.domain.model.aggregate.ServerWalletBuilder;
import se.digg.wallet.r2ps.domain.event.DeviceKeyAdded;
import se.digg.wallet.r2ps.domain.event.Event;
import se.digg.wallet.r2ps.domain.event.ServerWalletRegistered;

import java.security.PublicKey;
import java.time.Instant;
import java.util.List;
import java.util.Optional;
import java.util.UUID;
import java.util.stream.Collectors;
import java.util.stream.Stream;

@Service
public class ServerWalletEventApplier {

  public ServerWallet apply(Optional<ServerWallet> currentAggregate, Event e) {
    return switch (e) {
      case ServerWalletRegistered ev -> applyEvent(currentAggregate, ev);
      case ServerWalletRevoked ev -> applyEvent(currentAggregate, ev);
      case DeviceKeyAdded ev -> applyEvent(currentAggregate, ev);
      case DeviceKeyRevoked ev -> applyEvent(currentAggregate, ev);
      case HsmKeyCreated ev -> applyEvent(currentAggregate, ev);
      case HsmKeyDeleted ev -> applyEvent(currentAggregate, ev);

      default -> throw new IllegalStateException("Unexpected event type: " + e.getClass());
    };
  }

  private ServerWallet applyEvent(Optional<ServerWallet> currentAggregate, ServerWalletRegistered e) {
    if (currentAggregate.isPresent()) {
      return currentAggregate.get();
    }

    if (e.metadata().version() != 1) {
      throw new VersionConflict("ServerWalletRegistered has to be the first event");
    }

    return ServerWalletBuilder.builder().
        walletId(e.metadata().walletId()).
        deviceKeys(List.of()).
        hsmKeys(List.of()).
        created(e.metadata().timestamp()).
        updated(e.metadata().timestamp()).
        version(e.metadata().version()).
        build();
  }

  private ServerWallet applyEvent(Optional<ServerWallet> currentAggregate, ServerWalletRevoked serverWalletRevoked) {
    if (currentAggregate.isPresent() && validateBeforeWrite(currentAggregate.get(), serverWalletRevoked)) {
      return ServerWalletBuilder.builder(currentAggregate.get()).revoked(Optional.of(serverWalletRevoked.metadata().timestamp())).build();
    }
    throw new WalletNotFoundException(String.format("Cannot apply %s to non-existing ServerWallet aggregate %s", serverWalletRevoked.metadata().eventType() ,serverWalletRevoked.metadata().walletId()));
  }

  private ServerWallet applyEvent(Optional<ServerWallet> currentAggregate, DeviceKeyAdded deviceKeyAdded) {
    if (currentAggregate.isPresent() && validateBeforeWrite(currentAggregate.get(), deviceKeyAdded)) {
          if (currentAggregate.get().deviceKeys().stream().anyMatch(item -> item.deviceId().equals(deviceKeyAdded.deviceId()))) {
            // ignore duplicate device key addition
            return currentAggregate.get();
          }
          Stream<DeviceKey> oldDeviceKeys = currentAggregate.get().deviceKeys().stream();
          DeviceKey newDeviceKey = DeviceKeyBuilder.builder()
              .walletId(deviceKeyAdded.metadata().walletId())
              .deviceId(deviceKeyAdded.deviceId())
              .devicePublicKey(deviceKeyAdded.devicePublicKey())
              .revoked(Optional.empty())
              .created(deviceKeyAdded.metadata().timestamp())
              .updated(deviceKeyAdded.metadata().timestamp())
              .build();
          List<DeviceKey> newDeviceKeys = Stream.concat(oldDeviceKeys, Stream.of(newDeviceKey)).toList();

          return ServerWalletBuilder.builder(currentAggregate.get()).deviceKeys(newDeviceKeys).build();
    }
    throw new WalletNotFoundException(String.format("Cannot apply %s to non-existing ServerWallet aggregate %s", deviceKeyAdded.metadata().eventType() ,deviceKeyAdded.metadata().walletId()));
  }

  private ServerWallet applyEvent(Optional<ServerWallet> currentAggregate, DeviceKeyRevoked deviceKeyRevoked) {
    if (currentAggregate.isPresent() && validateBeforeWrite(currentAggregate.get(), deviceKeyRevoked)) {
      List<DeviceKey> oldDeviceKeys = currentAggregate.get().deviceKeys();
      List<DeviceKey> newDeviceKeys = oldDeviceKeys.stream().map(item -> {
        if (deviceKeyRevoked.deviceId().equals(item.deviceId())) {
          return DeviceKeyBuilder.builder(item).revoked(Optional.of(deviceKeyRevoked.metadata().timestamp())).build();
        }
        return item;
      }).toList();

      return ServerWalletBuilder.builder(currentAggregate.get()).deviceKeys(newDeviceKeys).build();
    }
    throw new WalletNotFoundException(String.format("Cannot apply %s to non-existing ServerWallet aggregate %s", deviceKeyRevoked.metadata().eventType(), deviceKeyRevoked.metadata().walletId()));
  }

  private ServerWallet applyEvent(Optional<ServerWallet> currentAggregate, HsmKeyCreated hsmKeyCreated) {
    if (currentAggregate.isPresent() && validateBeforeWrite(currentAggregate.get(), hsmKeyCreated)) {
      List<HsmKey> oldHsmKeys = currentAggregate.get().hsmKeys();
      HsmKey newHsmKey = HsmKeyBuilder.builder()
          .walletId(hsmKeyCreated.metadata().walletId())
          .keyId(hsmKeyCreated.kid())
          .curveName(hsmKeyCreated.curveName())
          .publicKey(hsmKeyCreated.publicKey())
          .created(hsmKeyCreated.metadata().timestamp())
          .build();
      List<HsmKey> newHsmKeys = Stream.concat(oldHsmKeys.stream(),
          Stream.of(newHsmKey)).toList();
      return ServerWalletBuilder.builder(currentAggregate.get()).hsmKeys(newHsmKeys).build();
    }
    throw new WalletNotFoundException(String.format("Cannot apply %s to non-existing ServerWallet aggregate %s", hsmKeyCreated.metadata().eventType(), hsmKeyCreated.metadata().walletId()));
  }

  private ServerWallet applyEvent(Optional<ServerWallet> currentAggregate, HsmKeyDeleted hsmKeyDeleted) {
    if (currentAggregate.isPresent() && validateBeforeWrite(currentAggregate.get(), hsmKeyDeleted)) {
      List<HsmKey> oldHsmKeys = currentAggregate.get().hsmKeys();
      List<HsmKey> newHsmKeys = oldHsmKeys.stream().filter(item -> !hsmKeyDeleted.keyId().equals(item.keyId())).toList();
      return ServerWalletBuilder.builder(currentAggregate.get()).hsmKeys(newHsmKeys).build();
    }
    throw new WalletNotFoundException(String.format("Cannot apply %s to non-existing ServerWallet aggregate %s", hsmKeyDeleted.metadata().eventType(), hsmKeyDeleted.metadata().walletId()));
  }

  private boolean validateBeforeWrite(ServerWallet currentAggregate, Event event) {
    if (event.metadata().version() <= currentAggregate.version()) {
      // Idempotent operation - event is applied with at least once semantics
      return false;
    }
    if (event.metadata().version() != (currentAggregate.version() + 1) ) {
      // Only next version in sequence is allowed - no gaps
      throw new VersionConflict(String.format("Event version %d is not valid for ServerWallet aggregate %s with current version %d",
          event.metadata().version(),
          event.metadata().walletId(),
          currentAggregate.version()));
    }
    return true;
  }
}
