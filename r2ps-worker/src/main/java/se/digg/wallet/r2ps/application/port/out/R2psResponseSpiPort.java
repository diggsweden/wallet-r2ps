package se.digg.wallet.r2ps.application.port.out;

import se.digg.wallet.r2ps.domain.domain.model.R2psResponse;

public interface R2psResponseSpiPort {
  void r2psResponse(R2psResponse r2psResponse);
}
