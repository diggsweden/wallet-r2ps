package se.digg.wallet.r2ps.application.port.in;

import se.digg.wallet.r2ps.domain.model.R2psRequest;

public interface R2psRequestUseCase {
  void r2psRequest(R2psRequest r2psRequest);
}
