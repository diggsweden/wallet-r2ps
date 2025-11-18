package se.digg.wallet.r2ps.domain.command;

import java.time.Instant;
import java.util.Optional;
import java.util.UUID;

public record CommandMetadata(UUID commandId, UUID walletId, String commandType, Instant timestamp, Optional<String> basedOnVersion, Optional<UUID> correlationId) {
}
