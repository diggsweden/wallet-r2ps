package se.digg.wallet.r2ps.application.port.out;


import se.digg.wallet.r2ps.domain.model.aggregate.ServerWallet;

import java.util.Optional;
import java.util.UUID;

public interface ServerWalletRepository {
  ServerWallet save(ServerWallet wallet);
  Optional<ServerWallet> findById(UUID id);
  boolean existsById(UUID id);
}
