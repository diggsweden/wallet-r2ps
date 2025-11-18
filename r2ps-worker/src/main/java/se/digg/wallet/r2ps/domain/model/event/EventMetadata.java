package se.digg.wallet.r2ps.domain.domain.model.event;

import java.time.Instant;

public record EventMetadata(String eventId, Instant timestamp, String version) {
}

