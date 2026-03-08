package se.digg.wallet.r2ps.domain.model;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;
import java.util.Optional;
import java.util.UUID;

@JsonIgnoreProperties(ignoreUnknown = true)
public record R2psResponse(
    UUID correlationId, int httpStatus, Optional<String> stateJws, String serviceResponseJws) {}
