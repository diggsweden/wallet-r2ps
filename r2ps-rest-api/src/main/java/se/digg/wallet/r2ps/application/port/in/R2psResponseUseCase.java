package se.digg.wallet.r2ps.application.port.in;

import se.digg.wallet.r2ps.domain.model.R2psResponse;
import se.digg.wallet.r2ps.infrastructure.adapter.dto.R2psResponseDto;

import java.util.Optional;
import java.util.UUID;

public interface R2psResponseUseCase {
  void r2psResponseReady(R2psResponse r2psResponse);

  Optional<R2psResponse> waitForR2psResponse(UUID correlationId, long timeoutMillis);
}
