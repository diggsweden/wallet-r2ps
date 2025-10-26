package se.digg.wallet.r2ps.application.mapper;

import org.springframework.stereotype.Component;
import se.digg.wallet.r2ps.application.dto.command.AddDeviceKey;
import se.digg.wallet.r2ps.application.dto.command.Command;
import se.digg.wallet.r2ps.application.dto.command.CommandMetadata;
import se.digg.wallet.r2ps.application.dto.command.CreateHsmKey;
import se.digg.wallet.r2ps.application.dto.command.DeleteHsmKey;
import se.digg.wallet.r2ps.application.dto.command.RegisterServerWallet;
import se.digg.wallet.r2ps.application.dto.command.RevokeDeviceKey;
import se.digg.wallet.r2ps.application.dto.command.RevokeServerWallet;
import se.digg.wallet.r2ps.domain.exception.DeviceAlreadyExistsException;
import se.digg.wallet.r2ps.domain.exception.DeviceNotFoundException;
import se.digg.wallet.r2ps.domain.exception.HsmKeyAlreadyExistsException;
import se.digg.wallet.r2ps.domain.exception.WalletAlreadyExistsException;
import se.digg.wallet.r2ps.domain.exception.WalletInternalServerException;
import se.digg.wallet.r2ps.domain.exception.WalletNotFoundException;
import se.digg.wallet.r2ps.domain.model.aggregate.ServerWallet;
import se.digg.wallet.r2ps.domain.event.DeviceKeyAdded;
import se.digg.wallet.r2ps.domain.event.DeviceKeyRevoked;
import se.digg.wallet.r2ps.domain.event.Event;
import se.digg.wallet.r2ps.domain.event.EventMetadata;
import se.digg.wallet.r2ps.domain.event.HsmKeyCreated;
import se.digg.wallet.r2ps.domain.event.HsmKeyDeleted;
import se.digg.wallet.r2ps.domain.event.ServerWalletRegistered;
import se.digg.wallet.r2ps.domain.event.ServerWalletRevoked;

import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.security.PublicKey;
import java.util.Base64;
import java.util.Optional;

@Component
public class CommandToEventsMapper {

  public Event handleCommand(Command cm, Optional<ServerWallet> previousVersion) {
    return switch (cm) {
      case RegisterServerWallet c -> mapTo(c, previousVersion);
      case RevokeServerWallet c -> mapTo(c, previousVersion);
      case AddDeviceKey c -> mapTo(c, previousVersion);
      case RevokeDeviceKey c -> mapTo(c, previousVersion);
      case CreateHsmKey c -> mapTo(c, previousVersion);
      case DeleteHsmKey c -> mapTo(c, previousVersion);
      default -> throw new IllegalStateException("Unexpected value: " + cm);
    };
  }

  private ServerWalletRegistered mapTo(RegisterServerWallet  c, Optional<ServerWallet> previousVersion) {
    if (previousVersion.isPresent()) {
      throw new WalletAlreadyExistsException("Wallet with walletId {} already exists", c.metadata().walletId());
    }
    return new ServerWalletRegistered(mapTo(c.metadata(), 0, ServerWalletRegistered.class.getSimpleName()));
  }

  private ServerWalletRevoked mapTo(RevokeServerWallet c, Optional<ServerWallet> previousVersion) {
    if (previousVersion.isEmpty()) {
      throw new WalletNotFoundException("Cannot add device to non-existing wallet walletId {}", c.metadata().walletId());
    }

    return new ServerWalletRevoked(mapTo(c.metadata(), previousVersion.get().version(), ServerWalletRevoked.class.getSimpleName()));
  }

  private DeviceKeyAdded mapTo(AddDeviceKey c, Optional<ServerWallet> previousVersion) {
    if (previousVersion.isEmpty()) {
        throw new WalletNotFoundException("Cannot add device to non-existing wallet walletId {}", c.metadata().walletId());
    }

    final String deviceId;
    try {
      deviceId = generateThumbprint(c.devicePublicKey());
    } catch (NoSuchAlgorithmException e) {
      throw  new WalletInternalServerException(e.getMessage());
    }

    previousVersion.get().device(deviceId).ifPresent(d -> {
      throw new DeviceAlreadyExistsException("DeviceKey with deviceId {} already exists in wallet", c.deviceId());
    });

    return new DeviceKeyAdded(deviceId, c.devicePublicKey(), mapTo(c.metadata(), previousVersion.get().version(), DeviceKeyAdded.class.getSimpleName()));
  }

  private DeviceKeyRevoked mapTo(RevokeDeviceKey c, Optional<ServerWallet> previousVersion) {
    if (previousVersion.isEmpty()) {
      throw new WalletNotFoundException("Cannot revoke device from non-existing wallet {}", c.metadata().walletId());
    }

    if (previousVersion.get().device(c.deviceId()).isEmpty()) {
      throw new DeviceNotFoundException("DeviceKey with deviceId {} is not found in wallet ", c.deviceId());
    };

    return new DeviceKeyRevoked(c.deviceId(), mapTo(c.metadata(), previousVersion.get().version(), DeviceKeyRevoked.class.getSimpleName()) );
  }

  private HsmKeyCreated mapTo(CreateHsmKey c, Optional<ServerWallet> previousVersion) {
    if (previousVersion.isEmpty()) {
      throw new WalletNotFoundException("Cannot create HSM key for non-existing wallet {}", c.metadata().walletId());
    }

    final String kid;
    try {
      kid = generateThumbprint(c.publicKey());
    } catch (NoSuchAlgorithmException e) {
      throw  new WalletInternalServerException(e.getMessage());
    }

    previousVersion.get().hsmKeyByKeyId(kid).ifPresent(d -> {
      throw new HsmKeyAlreadyExistsException("HsmKey with id {} already exists in wallet",  kid);
    });

    return new HsmKeyCreated(kid, c.curveName(), c.creationTime(), c.publicKey(),
        mapTo(c.metadata(), previousVersion.get().version(), HsmKeyCreated.class.getSimpleName()));
  }

  private HsmKeyDeleted mapTo(DeleteHsmKey c, Optional<ServerWallet> previousVersion) {
    if (previousVersion.isEmpty()) {
      throw new WalletNotFoundException("Cannot create HSM key for non-existing wallet {}", c.metadata().walletId());
    }

    if (previousVersion.get().hsmKeyByKeyId(c.keyId()).isEmpty()) {
      throw new HsmKeyAlreadyExistsException("HsmKey with kid {} does not exist in wallet",  c.keyId());
    }

    return new HsmKeyDeleted(c.keyId(),
        mapTo(c.metadata(), previousVersion.get().version(), HsmKeyDeleted.class.getSimpleName()));
  }

  private EventMetadata mapTo(CommandMetadata cm, int previousVersionNo, String eventType) {
    return new EventMetadata(cm.commandId().toString(), cm.walletId(), eventType, cm.timestamp(), cm.correlationId(), previousVersionNo + 1 );
  }

  private String generateThumbprint(PublicKey publicKey)
      throws NoSuchAlgorithmException {
    // Get the encoded form of the public key (X.509 format)
    byte[] encoded = publicKey.getEncoded();

    // Hash it with SHA-256
    MessageDigest digest = MessageDigest.getInstance("SHA-256");
    byte[] hash = digest.digest(encoded);

    // Encode as Base64URL (no padding, URL-safe)
    return Base64.getUrlEncoder().withoutPadding().encodeToString(hash);
  }
}
