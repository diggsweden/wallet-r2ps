package se.digg.wallet.r2ps.application.service;

import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.databind.ObjectMapper;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.stereotype.Service;
import se.digg.wallet.r2ps.application.port.in.R2psRequestUseCase;
import se.digg.wallet.r2ps.application.port.out.R2psResponseSpiPort;
import se.digg.wallet.r2ps.commons.StaticResources;
import se.digg.wallet.r2ps.commons.dto.ErrorCode;
import se.digg.wallet.r2ps.commons.dto.ErrorResponse;
import se.digg.wallet.r2ps.commons.exception.ServiceRequestHandlingException;
import se.digg.wallet.r2ps.domain.model.R2psRequest;
import se.digg.wallet.r2ps.domain.model.R2psResponse;
import se.digg.wallet.r2ps.server.service.ServiceRequestHandler;

import java.util.List;

@Service
public class R2psProcessService implements R2psRequestUseCase {

  private static final Logger log = LoggerFactory.getLogger(R2psProcessService.class);
  private final R2psResponseSpiPort r2psResponseSpiPort;

  private final ObjectMapper objectMapper;

  private final ServiceRequestHandler serviceRequestHandler;

  public R2psProcessService(R2psResponseSpiPort r2psResponseSpiPort, ObjectMapper objectMapper,
      ServiceRequestHandler serviceRequestHandler) {
    this.r2psResponseSpiPort = r2psResponseSpiPort;
    this.objectMapper = objectMapper;
    this.serviceRequestHandler = serviceRequestHandler;
  }

  @Override
  public R2psResponse r2psRequest(R2psRequest r2psRequest) {

    R2psResponse r2psResponse;

    try {
      String responseBody = serviceRequestHandler.handleServiceRequest(r2psRequest.payload());
      log.info("R2PS request handled successfully for requestId: {}", r2psRequest.requestId());
      r2psResponse = new R2psResponse(
          r2psRequest.requestId(),
          r2psRequest.walletId(),
          r2psRequest.deviceId(),
          200,
          responseBody);
    } catch (ServiceRequestHandlingException e) {
      log.error("R2PS request failed for requestId: {} error: ", r2psRequest.requestId(), e);
      r2psResponse = getErrorResponseString(r2psRequest, e.getErrorCode(), e.getMessage());
    } catch (RuntimeException re) {
      log.error("Unexpected error while processing R2PS request for requestId: {} ",
          r2psRequest.requestId(), re);
      throw re;
    }


    return r2psResponse;
  }

  private R2psResponse getErrorResponseString(R2psRequest r2psRequest, ErrorCode errorCode,
      String message) {
    try {
      String body = objectMapper.writeValueAsString(ErrorResponse.builder()
          .errorCode(errorCode.name())
          .message(message)
          .build());
      return new R2psResponse(r2psRequest.requestId(),
          r2psRequest.walletId(),
          r2psRequest.deviceId(),
          errorCode.getResponseCode(),
          r2psRequest.payload());
    } catch (JsonProcessingException e) {
      throw new RuntimeException(e);
    }
  }
}
