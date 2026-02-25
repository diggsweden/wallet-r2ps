package se.digg.wallet.r2ps.domain.model;

import java.util.Optional;
import java.util.UUID;

public record R2psResponse(UUID requestId, int httpStatus, Optional<String> stateJws, String serviceResponseJws) {
}
