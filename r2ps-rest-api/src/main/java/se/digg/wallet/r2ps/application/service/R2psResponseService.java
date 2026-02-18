package se.digg.wallet.r2ps.application.service;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import se.digg.wallet.r2ps.application.dto.PendingRequestContext;
import se.digg.wallet.r2ps.application.port.in.R2psResponseUseCase;
import se.digg.wallet.r2ps.application.port.out.PendingRequestContextSpiPort;
import se.digg.wallet.r2ps.application.port.out.R2psDeviceStateSpiPort;
import se.digg.wallet.r2ps.application.port.out.R2psResponseSinkSpiPort;
import se.digg.wallet.r2ps.domain.model.R2psResponse;

import java.util.Optional;
import java.util.UUID;

import static java.lang.Thread.sleep;

public class R2psResponseService implements R2psResponseUseCase {

  private static final Logger log = LoggerFactory.getLogger(R2psResponseService.class);
  private final R2psResponseSinkSpiPort r2psResponseSinkSpiPort;
  private final R2psDeviceStateSpiPort r2psDeviceStateSpiPort;
  private final PendingRequestContextSpiPort pendingRequestContextSpiPort;

  public R2psResponseService(R2psResponseSinkSpiPort r2psResponseSinkSpiPort,
      R2psDeviceStateSpiPort r2psDeviceStateSpiPort,
      PendingRequestContextSpiPort pendingRequestContextSpiPort) {
    this.r2psResponseSinkSpiPort = r2psResponseSinkSpiPort;
    this.r2psDeviceStateSpiPort = r2psDeviceStateSpiPort;
    this.pendingRequestContextSpiPort = pendingRequestContextSpiPort;
  }

  @Override
  public void r2psResponseReady(R2psResponse r2psResponse) {
    PendingRequestContext ctx = pendingRequestContextSpiPort.load(r2psResponse.requestId().toString())
        .orElseThrow(() -> new IllegalStateException(
            "No pending context for requestId: " + r2psResponse.requestId()));
    r2psDeviceStateSpiPort.save(ctx.stateKey(), r2psResponse.stateJws(), ctx.ttlSeconds());
    r2psResponseSinkSpiPort.storeResponse(r2psResponse);
  }

  @Override
  public Optional<R2psResponse> waitForR2psResponse(UUID requestId, long timeoutMillis) {
    long endTime = System.currentTimeMillis() + timeoutMillis;

    try {
      while (System.currentTimeMillis() < endTime) {
        Optional<R2psResponse> r2psResponse = r2psResponseSinkSpiPort.loadResponse(requestId);
        if (r2psResponse.isPresent()) {
          log.info("Got r2psResponseDto for {}", requestId);
          return r2psResponse;
        }
        sleep(100); // poll interval
      }
    } catch (InterruptedException e) {
      log.info("Interrupted while waiting for register wallet response for requestId: {}",
          requestId);
    }
    return Optional.empty();
  }
}
