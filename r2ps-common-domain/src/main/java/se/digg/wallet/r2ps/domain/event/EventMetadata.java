package se.digg.wallet.r2ps.domain.event;

import com.fasterxml.jackson.annotation.JsonInclude;
import io.soabase.recordbuilder.core.RecordBuilder;

import java.time.Instant;
import java.util.Optional;
import java.util.UUID;

@RecordBuilder
@JsonInclude(JsonInclude.Include.NON_NULL)
public record EventMetadata(String eventId, UUID walletId, String eventType, Instant timestamp, Optional<UUID> correlationId, int version) {
}

