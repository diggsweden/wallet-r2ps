package se.digg.wallet.r2ps.infrastructure.adapter.dto;

import io.soabase.recordbuilder.core.RecordBuilder;
import se.digg.wallet.r2ps.domain.event.Event;

import java.util.List;
import java.util.UUID;

@RecordBuilder
public record R2psResponseDto(UUID requestId, UUID walletId, UUID deviceId, int httpStatus,
    String payload,
    String pakeSessionId, List<Event> events) {
}
