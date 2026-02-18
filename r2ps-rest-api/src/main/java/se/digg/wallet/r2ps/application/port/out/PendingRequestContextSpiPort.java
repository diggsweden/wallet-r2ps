package se.digg.wallet.r2ps.application.port.out;

import se.digg.wallet.r2ps.application.dto.PendingRequestContext;

import java.util.Optional;

public interface PendingRequestContextSpiPort {
  void save(String requestId, PendingRequestContext ctx);

  Optional<PendingRequestContext> load(String requestId);
}
