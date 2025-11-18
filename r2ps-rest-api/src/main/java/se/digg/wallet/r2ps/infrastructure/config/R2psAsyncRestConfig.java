package se.digg.wallet.r2ps.infrastructure.config;

public record R2psAsyncRestConfig(String responseTemplateUrl, boolean serveSync,
    long syncTimeoutMs) {
}
