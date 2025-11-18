package se.digg.wallet.r2ps.infrastructure.adapter.out.persistence;

import org.springframework.data.jpa.repository.JpaRepository;
import se.digg.wallet.r2ps.infrastructure.adapter.out.persistence.entity.ServerWalletEntity;

import java.util.UUID;

public interface JpaServerWalletRepository extends JpaRepository<ServerWalletEntity, UUID> {
}

