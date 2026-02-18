package se.digg.wallet.r2ps.domain.model;

import java.util.UUID;

public record R2psResponse(UUID requestId, int httpStatus, String stateJws, String serviceResponseJws) {
}
