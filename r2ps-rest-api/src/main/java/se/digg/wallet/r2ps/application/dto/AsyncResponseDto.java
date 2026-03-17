package se.digg.wallet.r2ps.application.dto;

import com.fasterxml.jackson.annotation.JsonInclude;
import io.soabase.recordbuilder.core.RecordBuilder;

import java.net.URI;
import java.util.Optional;

@RecordBuilder
@JsonInclude(JsonInclude.Include.NON_EMPTY)
public record AsyncResponseDto<T>(
    String correlationId,
    AsyncResponseStatus status,
    Optional<T> result,
    Optional<URI> resultUrl,
    Optional<AsyncResponseError> error) {
}
