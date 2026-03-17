package se.digg.wallet.r2ps.application.dto;

/**
 * Maps a correlationId to the deviceId for response routing.
 * State is no longer stored by the BFF — the worker owns state.
 */
public record PendingRequestContext(String deviceId) {}
