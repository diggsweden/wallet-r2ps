package se.digg.wallet.r2ps.infrastructure.adapter.out.persistence;

import com.fasterxml.jackson.databind.ObjectMapper;
import org.springframework.stereotype.Repository;
import se.digg.wallet.r2ps.domain.aggregate.ServerWallet;
import se.digg.wallet.r2ps.application.port.out.DeviceKeyRegistrySpiPort;
import se.digg.wallet.r2ps.infrastructure.adapter.out.persistence.entity.ServerWalletEntity;
import se.digg.wallet.r2ps.server.service.ClientPublicKeyRecord;

import java.util.Optional;
import java.util.UUID;

@Repository
public class ServerWalletRegistry {

  private final JpaServerWalletRepository jpaRepository;
  private final ObjectMapper objectMapper;

  public ServerWalletRegistry(
      JpaServerWalletRepository jpaRepository,
      ObjectMapper objectMapper) {
    this.jpaRepository = jpaRepository;
    this.objectMapper = objectMapper;
  }

  public ServerWallet save(ServerWallet wallet) {
    ServerWalletEntity entity = toEntity(wallet);
    ServerWalletEntity saved = jpaRepository.save(entity);
    return toDomain(saved);
  }

  public Optional<ServerWallet> findById(UUID id) {
    return jpaRepository.findById(id).map(this::toDomain);
  }

  public boolean existsById(UUID id) {
    return jpaRepository.existsById(id);
  }

  private ServerWalletEntity toEntity(ServerWallet wallet) {
    return new ServerWalletEntity(
        wallet.walletId(),
        wallet);
  }

  private ServerWallet toDomain(ServerWalletEntity entity) {
    return objectMapper.convertValue(entity.getData(), ServerWallet.class);
  }

}
