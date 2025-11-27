package se.digg.wallet.r2ps.application.port.out;

import se.digg.wallet.r2ps.domain.model.R2psResponse;
import se.digg.wallet.r2ps.infrastructure.adapter.dto.R2psResponseDto;

import java.util.Optional;
import java.util.UUID;

public interface R2psResponseSinkSpiPort {
  void storeResponse(R2psResponse r2psResponse);

  Optional<R2psResponse> loadResponse(UUID requestId);
}
