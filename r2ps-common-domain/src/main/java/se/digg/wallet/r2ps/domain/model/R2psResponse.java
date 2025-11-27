package se.digg.wallet.r2ps.domain.model;

import java.util.UUID;

public record R2psResponse(UUID requestId, UUID walletId, UUID deviceId, int httpStatus, String payload) {
}
