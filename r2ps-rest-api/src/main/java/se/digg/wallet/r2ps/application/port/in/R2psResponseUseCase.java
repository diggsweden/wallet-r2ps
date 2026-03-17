package se.digg.wallet.r2ps.application.port.in;

import java.util.Optional;
import se.digg.wallet.r2ps.domain.model.R2psResponse;

public interface R2psResponseUseCase {
  void r2psResponseReady(R2psResponse r2psResponse);

  Optional<R2psResponse> waitForR2psResponse(String correlationId, long timeoutMillis);
}
