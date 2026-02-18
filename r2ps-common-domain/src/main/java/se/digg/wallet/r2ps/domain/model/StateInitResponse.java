package se.digg.wallet.r2ps.domain.model;

public record StateInitResponse(
    String requestId, String stateJws, String devAuthorizationCode) {}
