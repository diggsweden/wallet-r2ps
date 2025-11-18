package se.digg.wallet.r2ps.infrastructure.adapter.dto;

import io.soabase.recordbuilder.core.RecordBuilder;
import se.digg.wallet.r2ps.domain.domain.model.event.Event;

import java.util.List;
import java.util.UUID;

@RecordBuilder
public record R2psResponseDto(UUID walletId, UUID requestId, UUID deviceId, int httpStatus,
    String payload,
    String pakeSessionId, List<Event> events) {
}
