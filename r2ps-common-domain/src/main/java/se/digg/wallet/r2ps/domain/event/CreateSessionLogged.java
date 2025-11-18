package se.digg.wallet.r2ps.domain.event;

public record CreateSessionLogged(String sessionId, String context, String purpose, int ttlSeconds, EventMetadata metadata) implements Event {
}
