package se.digg.wallet.r2ps.infrastructure.adapter.dto;

import com.fasterxml.jackson.annotation.JsonProperty;
import io.soabase.recordbuilder.core.RecordBuilder;

@RecordBuilder
public record ErrorMessageDto(String errorCode, String message) {
}
