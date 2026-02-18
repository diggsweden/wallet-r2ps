package se.digg.wallet.r2ps.application.dto;

public record PendingRequestContext(String stateKey, long ttlSeconds) {}
