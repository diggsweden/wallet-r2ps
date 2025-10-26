package se.digg.wallet.r2ps.infrastructure.adapter.in.web.dto;

import io.soabase.recordbuilder.core.RecordBuilder;

import java.util.UUID;

@RecordBuilder
public record R2psInitRequestResponseDto(UUID requestId) {
}
