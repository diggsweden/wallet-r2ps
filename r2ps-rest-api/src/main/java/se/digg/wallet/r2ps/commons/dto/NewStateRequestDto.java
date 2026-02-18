package se.digg.wallet.r2ps.commons.dto;

import se.digg.wallet.r2ps.domain.model.EcPublicJwk;

/**
 * DEV-ONLY: overwrite and clientId fields must be removed before production.
 */
public record NewStateRequestDto(
    EcPublicJwk publicKey,
    String clientId,   // DEV-ONLY: supply existing clientId to overwrite state
    boolean overwrite, // DEV-ONLY: must be removed in production
    String ttl         // ISO 8601 duration, e.g. "P30D" or "PT1H"; null = server default
) {}
