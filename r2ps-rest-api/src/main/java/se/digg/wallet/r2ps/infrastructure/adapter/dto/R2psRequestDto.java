package se.digg.wallet.r2ps.infrastructure.adapter.dto;


import io.soabase.recordbuilder.core.RecordBuilder;

import java.util.UUID;

@RecordBuilder
public record R2psRequestDto(UUID requestId, UUID walletId, UUID deviceId, String payload) {

}
