package se.digg.wallet.r2ps.infrastructure.adapter.in.web;

import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.nimbusds.jose.JWSObject;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.http.HttpStatus;
import org.springframework.http.MediaType;
import org.springframework.http.ResponseEntity;
import org.springframework.web.ErrorResponse;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RestController;
import se.digg.wallet.r2ps.application.port.in.R2psRequestUseCase;
import se.digg.wallet.r2ps.application.port.out.R2psRequestMessageSpiPort;
import se.digg.wallet.r2ps.infrastructure.adapter.dto.ErrorCode;
import se.digg.wallet.r2ps.infrastructure.adapter.dto.ErrorMessageDtoBuilder;
import se.digg.wallet.r2ps.infrastructure.adapter.dto.R2psRequestDto;
import se.digg.wallet.r2ps.infrastructure.adapter.dto.R2psResponseDto;
import se.digg.wallet.r2ps.infrastructure.adapter.in.messaging.R2psResponseSink;


import java.text.ParseException;
import java.util.UUID;

@RestController
public class R2psRequestController {

  private final ObjectMapper objectMapper;

  private static final Logger log = LoggerFactory.getLogger(R2psRequestController.class);
  private final R2psRequestMessageSpiPort r2psRequestMessageSpiPort;
  private final R2psResponseSink r2psResponseSink;
  @Autowired
  public R2psRequestController(ObjectMapper objectMapper, final R2psRequestMessageSpiPort r2psRequestMessageSpiPort,
      R2psResponseSink r2psResponseSink) {
    this.objectMapper = objectMapper;
    this.r2psRequestMessageSpiPort = r2psRequestMessageSpiPort;
    this.r2psResponseSink = r2psResponseSink;
  }

  @PostMapping(value = "/service", produces = MediaType.APPLICATION_JSON_VALUE,
      consumes = MediaType.APPLICATION_JSON_VALUE)
  public ResponseEntity<String> service(@RequestBody final String serviceRequest) {

    try {
      if (log.isDebugEnabled()) {
        logServiceRequest(serviceRequest);
      }
      UUID requestId = UUID.randomUUID();
      String walletId = "3f9e0db4-4c83-4e18-b958-799aa400393a"; // TODO
      R2psRequestDto r2psRequestDto = new R2psRequestDto(requestId, walletId, serviceRequest);
      r2psRequestMessageSpiPort.sendR2psRequestMessage(r2psRequestDto);

      R2psResponseDto r2psResponseDto =
          r2psResponseSink.waitForSecureChannelResponse(requestId.toString(), 30);

      final String serviceResponse = r2psResponseDto.payload();
      final HttpStatus statusCode = HttpStatus.valueOf(r2psResponseDto.httpStatus());

      if (log.isDebugEnabled()) {
        logServiceResponse(serviceResponse);
      }
      return new ResponseEntity<>(serviceResponse, statusCode);
    } catch (InterruptedException e) {
      return getErrorResponseString(ErrorCode.SERVER_ERROR, e.getMessage());
    }
  }

  private void logServiceResponse(final String serviceResponse) {
    log.trace("Service response JWS: {}", serviceResponse);
    try {
      JWSObject jwsObject = JWSObject.parse(serviceResponse);
      log.trace("Received Service response:\n{}", objectMapper.writeValueAsString(
          jwsObject.getPayload().toJSONObject()
      ));
    } catch (JsonProcessingException | ParseException e) {
      throw new RuntimeException(e);
    }
  }

  private void logServiceRequest(final String serviceRequest) {
    log.trace("Service request JWS: {}", serviceRequest);
    try {
      JWSObject jwsObject = JWSObject.parse(serviceRequest);
      log.trace("Sending service request:\n{}", objectMapper.writeValueAsString(
          jwsObject.getPayload().toJSONObject()
      ));
    } catch (JsonProcessingException | ParseException e) {
      throw new RuntimeException(e);
    }
  }

  private ResponseEntity<String> getErrorResponseString(ErrorCode errorCode, String message) {
    try {
      String body = objectMapper.writeValueAsString(ErrorMessageDtoBuilder.builder()
          .errorCode(errorCode.name())
          .message(message)
          .build());
      return new ResponseEntity<>(body, HttpStatus.valueOf(errorCode.getResponseCode()));
    } catch (JsonProcessingException e) {
      throw new RuntimeException(e);
    }
  }
}
