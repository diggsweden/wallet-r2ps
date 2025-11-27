package se.digg.wallet.r2ps.infrastructure.adapter.in.web;

import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.nimbusds.jose.JWSObject;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.http.HttpStatus;
import org.springframework.http.MediaType;
import org.springframework.http.ResponseEntity;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PathVariable;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RestController;
import se.digg.wallet.r2ps.application.dto.AsyncResponseDto;
import se.digg.wallet.r2ps.application.dto.AsyncResponseError;
import se.digg.wallet.r2ps.application.dto.AsyncResponseStatus;
import se.digg.wallet.r2ps.application.port.in.R2psResponseUseCase;
import se.digg.wallet.r2ps.application.port.out.R2psRequestMessageSpiPort;
import se.digg.wallet.r2ps.application.port.out.R2psResponseSinkSpiPort;
import se.digg.wallet.r2ps.application.service.R2psResponseService;
import se.digg.wallet.r2ps.commons.dto.ServiceRequest;
import se.digg.wallet.r2ps.commons.exception.ServiceRequestHandlingException;
import se.digg.wallet.r2ps.domain.model.R2psRequest;
import se.digg.wallet.r2ps.domain.model.R2psResponse;
import se.digg.wallet.r2ps.infrastructure.adapter.dto.ErrorCode;
import se.digg.wallet.r2ps.infrastructure.adapter.dto.ErrorMessageDtoBuilder;
import se.digg.wallet.r2ps.infrastructure.adapter.dto.R2psRequestDto;
import se.digg.wallet.r2ps.infrastructure.adapter.dto.R2psResponseDto;
import se.digg.wallet.r2ps.infrastructure.adapter.in.messaging.R2psResponseReadyMessageReceiver;
import se.digg.wallet.r2ps.infrastructure.config.Config;
import se.digg.wallet.r2ps.infrastructure.service.UrlFormatterService;


import java.net.URI;
import java.text.ParseException;
import java.util.Optional;
import java.util.UUID;

@RestController
public class R2psRequestController {

  private final ObjectMapper objectMapper;

  private static final Logger log = LoggerFactory.getLogger(R2psRequestController.class);
  private final R2psRequestMessageSpiPort r2psRequestMessageSpiPort;
  private final R2psResponseUseCase r2psResponseUseCase;
  private final R2psResponseSinkSpiPort r2psResponseSinkSpiPort;
  private final UrlFormatterService urlFormatter;
  private final Config config;

  private final boolean syncResponseSupport;
  private final long maxResponseTimeoutInMillis;

  public R2psRequestController(ObjectMapper objectMapper,
      final R2psRequestMessageSpiPort r2psRequestMessageSpiPort,
      R2psResponseReadyMessageReceiver r2PsResponseReadyMessageReceiver,
      R2psResponseSinkSpiPort r2psResponseSinkSpiPort, UrlFormatterService urlFormatter,
      Config config) {
    this.objectMapper = objectMapper;
    this.r2psRequestMessageSpiPort = r2psRequestMessageSpiPort;
    this.r2psResponseSinkSpiPort = r2psResponseSinkSpiPort;
    this.urlFormatter = urlFormatter;
    this.config = config;
    syncResponseSupport = config.getKafka().rest().serveSync();
    maxResponseTimeoutInMillis = config.getKafka().rest().syncTimeoutMs();
    r2psResponseUseCase = new R2psResponseService(r2psResponseSinkSpiPort);
  }

  @GetMapping("/task/{correlationId}")
  public ResponseEntity<AsyncResponseDto<String>> taskResponse(
      @PathVariable("correlationId") UUID correlationId) {

    Optional<R2psResponse> r2psResponse =
        r2psResponseUseCase.waitForR2psResponse(correlationId, maxResponseTimeoutInMillis);
    if (r2psResponse.isEmpty()) {
      URI location = urlFormatter.responseEventsUrl(correlationId);
      AsyncResponseDto<String> responseBody =
          new AsyncResponseDto<>(correlationId, AsyncResponseStatus.PENDING, Optional.empty(),
              Optional.of(location), Optional.empty());
      log.info("registerResponseDto pending {}", responseBody);
      return ResponseEntity.accepted().location(location).body(responseBody);
    }

    if (r2psResponse.get().httpStatus() != HttpStatus.OK.value()) {
      Optional<AsyncResponseError> errorPayload = Optional.empty();
      try {
        ServiceRequestHandlingException serviceRequestHandlingException =
            objectMapper.readValue(r2psResponse.get().payload(),
                ServiceRequestHandlingException.class);
        if (serviceRequestHandlingException != null) {
          log.info("Parsed error payload: {}", serviceRequestHandlingException.getMessage());
          errorPayload =
              Optional.of(new AsyncResponseError(serviceRequestHandlingException.getMessage(),
                  serviceRequestHandlingException.getErrorCode().getResponseCode()));
        }
      } catch (JsonProcessingException e) {
        log.debug("Could not parse error payload", e);
      }
      AsyncResponseDto<String> registerResponseDto = new AsyncResponseDto<>(
          correlationId,
          AsyncResponseStatus.COMPLETE,
          Optional.empty(),
          Optional.empty(),
          errorPayload);
    }
    AsyncResponseDto<String> registerResponseDto = new AsyncResponseDto<>(
        correlationId,
        AsyncResponseStatus.COMPLETE,
        Optional.of(r2psResponse.get().payload()),
        Optional.empty(),
        Optional.empty());
    log.info("registerResponseDto {}", registerResponseDto);

    return ResponseEntity.ok(registerResponseDto);
  }

  @PostMapping(value = "/", produces = MediaType.APPLICATION_JSON_VALUE,
      consumes = MediaType.APPLICATION_JSON_VALUE)
  public ResponseEntity<AsyncResponseDto<String>> service(
      @RequestBody final String serviceRequestJws)
      throws ParseException, JsonProcessingException {
    if (log.isDebugEnabled()) {
      logServiceRequest(serviceRequestJws);
    }
    JWSObject jwsObject = JWSObject.parse(serviceRequestJws);
    ServiceRequest serviceRequest = (ServiceRequest) objectMapper
        .readValue(jwsObject.getPayload().toString(), ServiceRequest.class);

    UUID deviceId = UUID.fromString(serviceRequest.getClientID());
    String context = serviceRequest.getContext();
    UUID walletId = UUID.fromString(serviceRequest.getClientID()); // TODO

    UUID requestId = UUID.randomUUID();
    R2psRequest r2psRequest =
        new R2psRequest(requestId, walletId, deviceId, serviceRequestJws);
    log.info("Sending service request:\n{}", objectMapper.writeValueAsString(r2psRequest));
    r2psRequestMessageSpiPort.sendR2psRequestMessage(r2psRequest);

    if (syncResponseSupport) {
      log.info("Waiting for synchronous response for requestId: {}", requestId);
      return taskResponse(requestId);
    }

    URI location = urlFormatter.responseEventsUrl(requestId);
    AsyncResponseDto<String> responseBody =
        new AsyncResponseDto<String>(requestId, AsyncResponseStatus.PENDING, Optional.empty(),
            Optional.of(location), Optional.empty());
    return ResponseEntity.accepted().location(location).body(responseBody);
  }

  @PostMapping(value = "/service", produces = MediaType.APPLICATION_JSON_VALUE,
      consumes = MediaType.APPLICATION_JSON_VALUE)
  public ResponseEntity<String> legacySyncService(@RequestBody final String serviceRequestJws)
      throws ParseException, JsonProcessingException {
    ResponseEntity<AsyncResponseDto<String>> serviceResponse = this.service(serviceRequestJws);
    if (serviceResponse.getBody() != null && serviceResponse.getBody().result().isPresent()) {
      String body = serviceResponse.getBody().result().get();
      log.info("Response {} {}", serviceResponse.getStatusCode(), body);
      return ResponseEntity.status(serviceResponse.getStatusCode()).body(body);
    }
    return ResponseEntity.status(HttpStatus.REQUEST_TIMEOUT).build();
  }

  private void logServiceResponse(final String serviceResponse) {
    log.trace("Service response JWS: {}", serviceResponse);
    try {
      JWSObject jwsObject = JWSObject.parse(serviceResponse);
      log.trace("Received Service response:\n{}", objectMapper.writeValueAsString(
          jwsObject.getPayload().toJSONObject()));
    } catch (JsonProcessingException | ParseException e) {
      throw new RuntimeException(e);
    }
  }

  private void logServiceRequest(final String serviceRequest) {
    log.trace("Service request JWS: {}", serviceRequest);
    try {
      JWSObject jwsObject = JWSObject.parse(serviceRequest);
      log.trace("Sending service request:\n{}", objectMapper.writeValueAsString(
          jwsObject.getPayload().toJSONObject()));
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
