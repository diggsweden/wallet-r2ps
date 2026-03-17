package se.digg.wallet.r2ps.application.port.out;

import se.digg.wallet.r2ps.domain.model.R2psResponse;

import java.util.Optional;

public interface R2psResponseSinkSpiPort {
  void storeResponse(R2psResponse r2psResponse);

  Optional<R2psResponse> loadResponse(String correlationId);
}
