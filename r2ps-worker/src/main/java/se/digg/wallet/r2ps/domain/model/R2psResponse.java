package se.digg.wallet.r2ps.domain.domain.model;

import se.digg.wallet.r2ps.domain.domain.model.event.Event;

import java.util.List;
import java.util.UUID;

public record R2psResponse(UUID requestId, UUID walletId, UUID deviceId, int httpStatus,
    String payload,
    String pakeSessionId, List<Event> events) {
}
