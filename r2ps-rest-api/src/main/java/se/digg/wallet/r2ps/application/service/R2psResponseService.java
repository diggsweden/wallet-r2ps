package se.digg.wallet.r2ps.application.service;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import se.digg.wallet.r2ps.application.port.in.R2psResponseUseCase;
import se.digg.wallet.r2ps.application.port.out.R2psResponseSinkSpiPort;
import se.digg.wallet.r2ps.domain.model.R2psResponse;
import se.digg.wallet.r2ps.infrastructure.adapter.dto.R2psResponseDto;

import java.util.Optional;
import java.util.UUID;

import static java.lang.Thread.sleep;

public class R2psResponseService implements R2psResponseUseCase {

  private static final Logger log = LoggerFactory.getLogger(R2psResponseService.class);
  private final R2psResponseSinkSpiPort r2psResponseSinkSpiPort;

  public R2psResponseService(R2psResponseSinkSpiPort r2psResponseSinkSpiPort) {
    this.r2psResponseSinkSpiPort = r2psResponseSinkSpiPort;
  }

  @Override
  public void r2psResponseReady(R2psResponse r2psResponse) {
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
