package se.digg.wallet.r2ps.infrastructure.adapter.out.serverwallet;

import com.fasterxml.jackson.databind.ObjectMapper;
import org.springframework.stereotype.Repository;
import se.digg.wallet.r2ps.application.port.out.ServerWalletRepository;
import se.digg.wallet.r2ps.domain.model.aggregate.ServerWallet;
import se.digg.wallet.r2ps.infrastructure.adapter.out.serverwallet.entity.ServerWalletEntity;

import java.util.Optional;
import java.util.UUID;

@Repository
public class ServerWalletRepositoryAdapter implements ServerWalletRepository {

  private final JpaServerWalletRepository jpaRepository;
  private final ObjectMapper objectMapper;

  public ServerWalletRepositoryAdapter(
      JpaServerWalletRepository jpaRepository,
      ObjectMapper objectMapper
  ) {
    this.jpaRepository = jpaRepository;
    this.objectMapper = objectMapper;
  }

  @Override
  public ServerWallet save(ServerWallet wallet) {
    ServerWalletEntity entity = toEntity(wallet);
    ServerWalletEntity saved = jpaRepository.save(entity);
    return toDomain(saved);
  }

  @Override
  public Optional<ServerWallet> findById(UUID id) {
    return jpaRepository.findById(id).map(this::toDomain);
  }

  @Override
  public boolean existsById(UUID id) {
    return jpaRepository.existsById(id);
  }

  private ServerWalletEntity toEntity(ServerWallet wallet) {
    return new ServerWalletEntity(
        wallet.walletId(),
        wallet
    );
  }

  private ServerWallet toDomain(ServerWalletEntity entity) {
    return objectMapper.convertValue(entity.getData(), ServerWallet.class);
  }
}
