package se.digg.wallet.r2ps.infrastructure.adapter.out;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.stereotype.Service;
import se.digg.wallet.r2ps.domain.aggregate.DeviceKey;
import se.digg.wallet.r2ps.domain.aggregate.DeviceKeyBuilder;
import se.digg.wallet.r2ps.domain.aggregate.ServerWallet;
import se.digg.wallet.r2ps.domain.aggregate.ServerWalletBuilder;
import se.digg.wallet.r2ps.application.port.out.AuthorizationCodeSpiPort;
import se.digg.wallet.r2ps.application.port.out.DeviceKeyRegistrySpiPort;
import se.digg.wallet.r2ps.domain.exception.WalletNotFoundException;
import se.digg.wallet.r2ps.infrastructure.adapter.out.persistence.ServerWalletRegistry;
import se.digg.wallet.r2ps.server.service.ClientPublicKeyRecord;
import se.digg.wallet.r2ps.server.service.ClientPublicKeyRegistry;

import java.time.Instant;
import java.util.Base64;
import java.util.List;
import java.util.Optional;
import java.util.UUID;
import java.util.stream.Stream;

public class DeviceKeyRegistrySpiPortService
    implements DeviceKeyRegistrySpiPort, ClientPublicKeyRegistry {

  private static final Logger log = LoggerFactory.getLogger(DeviceKeyRegistrySpiPortService.class);
  private final AuthorizationCodeSpiPort authorizationCodeSpiPort;

  private final ServerWalletRegistry serverWalletRegistry;

  public DeviceKeyRegistrySpiPortService(AuthorizationCodeSpiPort authorizationCodeSpiPort,
      ServerWalletRegistry serverWalletRegistry) {
    this.authorizationCodeSpiPort = authorizationCodeSpiPort;
    this.serverWalletRegistry = serverWalletRegistry;
  }

  @Override
  public Optional<DeviceKey> getDeviceKey(UUID walletId, String kid) {
    Optional<ServerWallet> serverWallet = serverWalletRegistry.findById(walletId);
    return serverWallet.flatMap(wallet -> wallet.deviceKeys().stream()
        .filter(deviceKey -> deviceKey.kid().equals(kid))
        .findFirst());
  }

  @Override
  public ClientPublicKeyRecord getClientPublicKeyRecord(String clientId, String kid) {
    UUID walletId = UUID.fromString(clientId);
    Optional<DeviceKey> deviceKey = getDeviceKey(walletId, kid);
    byte[] authorization = null;
    if (deviceKey.isPresent()) {
      DeviceKey key = deviceKey.get();
      try {
        Optional<String> code = authorizationCodeSpiPort.getAuthorizationCode(walletId, kid);
        if (code.isPresent()) {
          authorization = Base64.getDecoder().decode(code.get());
        }
      } catch (Exception e) {
      }
      return ClientPublicKeyRecord.builder()
          .publicKey(key.devicePublicKey())
          .kid(key.kid())
          .supportedContexts(List.of("hsm"))
          .authorization(authorization)
          .build();
    }
    return null;
  }

  @Override
  public void registerClientPublicKey(String clientId,
      ClientPublicKeyRecord clientPublicKeyRecord) {
    ServerWallet previousVersion = serverWalletRegistry.findById(UUID.fromString(clientId))
        .orElseGet(() -> ServerWalletBuilder.builder()
            .walletId(UUID.fromString(clientId))
            .deviceKeys(List.of())
            .hsmKeys(List.of())
            .revoked(null)
            .created(Instant.now())
            .updated(Instant.now())
            .version(1)
            .build());

    List<DeviceKey> deviceKeys = Stream.concat(previousVersion.deviceKeys().stream(),
        Stream.of(DeviceKeyBuilder.builder()
            .deviceId(clientPublicKeyRecord.getKid())
            .devicePublicKeyBase64(Base64.getEncoder()
                .encodeToString(clientPublicKeyRecord.getPublicKey().getEncoded()))
            .build()))
        .toList();

    ServerWallet newVersion = ServerWalletBuilder.from(previousVersion)
        .withDeviceKeys(deviceKeys);
    serverWalletRegistry.save(newVersion);
  }

  @Override
  public void deleteClientPublicKeyRecord(String clientId, String kid) {
    Optional<ServerWallet> previousVersion =
        serverWalletRegistry.findById(UUID.fromString(clientId));

    if (previousVersion.isEmpty())
      throw new WalletNotFoundException("Unknown wallet {}", clientId);

    List<DeviceKey> deviceKeys = previousVersion.get().deviceKeys().stream().filter(
        dk -> !dk.kid().equals(kid)).toList();

    ServerWallet newVersion = ServerWalletBuilder.from(previousVersion.get())
        .withDeviceKeys(deviceKeys);
    serverWalletRegistry.save(newVersion);
  }

  @Override
  public boolean setAuthorizationCode(String clientId, String kid, byte[] authorizationCode) {
    log.info("setAuthorizationCode for clientId: {}, kid: {} code: {} {} {}", clientId, kid,
        authorizationCode, new String(authorizationCode), new String(authorizationCode).getBytes());
    try {
      String code = null;
      if (authorizationCode != null) {
        code = Base64.getEncoder().encodeToString(authorizationCode);
      }
      authorizationCodeSpiPort.setAuthorizationCode(
          UUID.fromString(clientId), kid, code);
      return true;
    } catch (Exception e) {
      return false;
    }
  }
}
