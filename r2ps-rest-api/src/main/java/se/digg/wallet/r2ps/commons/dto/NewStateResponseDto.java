package se.digg.wallet.r2ps.commons.dto;

public record NewStateResponseDto(
    String status,
    String clientId,
    String devAuthorizationCode
) {}
