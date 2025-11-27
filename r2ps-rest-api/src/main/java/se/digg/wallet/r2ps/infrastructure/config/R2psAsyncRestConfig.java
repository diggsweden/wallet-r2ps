package se.digg.wallet.r2ps.infrastructure.config;

public record R2psAsyncRestConfig(String responseEventsTemplateUrl,
    String responseWalletTemplateUrl, boolean serveSync,
    long syncTimeoutMs, long responseTtlSeconds) {
}
