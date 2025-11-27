package se.digg.wallet.r2ps.application.port.in;

import se.digg.wallet.r2ps.domain.model.R2psRequest;
import se.digg.wallet.r2ps.domain.model.R2psResponse;

public interface R2psRequestUseCase {
  R2psResponse r2psRequest(R2psRequest r2psRequest);
}
