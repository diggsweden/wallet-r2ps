package se.digg.wallet.r2ps.application.dto;

import com.fasterxml.jackson.annotation.JsonInclude;
import io.soabase.recordbuilder.core.RecordBuilder;

import java.net.URI;
import java.util.Optional;
import java.util.UUID;

@RecordBuilder
@JsonInclude(JsonInclude.Include.NON_EMPTY)
public record AsyncResponseDto<T>(
    UUID correlationId,
    AsyncResponseStatus status,
    Optional<T> result,
    Optional<URI> resultUrl,
    Optional<AsyncResponseError> error) {
}
