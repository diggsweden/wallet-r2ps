package se.digg.wallet.r2ps.application.service;

import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.databind.ObjectMapper;
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

  private final R2psResponseSpiPort r2psResponseSpiPort;

  private static final ObjectMapper objectMapper = StaticResources.SERVICE_EXCHANGE_OBJECT_MAPPER;

  private final ServiceRequestHandler serviceRequestHandler;

  public R2psProcessService(R2psResponseSpiPort r2psResponseSpiPort, ServiceRequestHandler serviceRequestHandler) {
    this.r2psResponseSpiPort = r2psResponseSpiPort;
    this.serviceRequestHandler = serviceRequestHandler;
  }

  @Override
  public R2psResponse r2psRequest(R2psRequest r2psRequest) {

    R2psResponse r2psResponse;
    try {
        r2psResponse = new R2psResponse(
            serviceRequestHandler.handleServiceRequest(r2psRequest.payload()),
            200
        );
    } catch (ServiceRequestHandlingException e) {
        r2psResponse = getErrorResponseString(e.getErrorCode(), e.getMessage());
    }

    r2psResponseSpiPort.r2psResponse(r2psResponse);

    return r2psResponse;
  }

  private R2psResponse getErrorResponseString(ErrorCode errorCode, String message) {
    try {
      String body = objectMapper.writeValueAsString(ErrorResponse.builder()
          .errorCode(errorCode.name())
          .message(message)
          .build());
      return new R2psResponse(body, errorCode.getResponseCode());
    } catch (JsonProcessingException e) {
      throw new RuntimeException(e);
    }
  }
}
