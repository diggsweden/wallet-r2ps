package se.digg.wallet.r2ps.infrastructure.adapter.out.serverwallet;

import org.springframework.data.jpa.repository.JpaRepository;
import se.digg.wallet.r2ps.infrastructure.adapter.out.serverwallet.entity.ServerWalletEntity;

import java.util.UUID;

public interface JpaServerWalletRepository extends JpaRepository<ServerWalletEntity, UUID> {
}

