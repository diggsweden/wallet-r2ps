package se.digg.wallet.r2ps.application.dto.command;

public record LogCreateSession(String sessionId, String context, String purpose, int ttlSeconds, CommandMetadata metadata)  {
}
