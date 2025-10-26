package se.digg.wallet.r2ps.domain.event;

import io.soabase.recordbuilder.core.RecordBuilder;

@RecordBuilder
public record ServerWalletRegistered(EventMetadata metadata) implements Event {
}
