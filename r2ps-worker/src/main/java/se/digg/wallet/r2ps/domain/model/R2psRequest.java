package se.digg.wallet.r2ps.domain.domain.model;

import java.util.UUID;

public record R2psRequest(UUID requestId, UUID walletId, UUID deviceId, String payload) {
}
