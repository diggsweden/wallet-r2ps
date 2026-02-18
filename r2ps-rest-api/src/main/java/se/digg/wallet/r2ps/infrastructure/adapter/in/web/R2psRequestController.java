package se.digg.wallet.r2ps.infrastructure.adapter.in.web;

import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.nimbusds.jose.JWSObject;
import java.net.URI;
import java.text.ParseException;
import java.time.Duration;
import java.util.Optional;
import java.util.UUID;
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
import se.digg.wallet.r2ps.application.dto.PendingRequestContext;
import se.digg.wallet.r2ps.application.port.in.R2psResponseUseCase;
import se.digg.wallet.r2ps.application.port.out.PendingRequestContextSpiPort;
import se.digg.wallet.r2ps.application.port.out.R2psDeviceStateSpiPort;
import se.digg.wallet.r2ps.application.port.out.R2psResponseSinkSpiPort;
import se.digg.wallet.r2ps.application.port.out.RequestMessageSpiPort;
import se.digg.wallet.r2ps.application.port.out.StateInitRequestSpiPort;
import se.digg.wallet.r2ps.application.service.R2psResponseService;
import se.digg.wallet.r2ps.commons.dto.BffRequest;
import se.digg.wallet.r2ps.commons.dto.NewStateRequestDto;
import se.digg.wallet.r2ps.commons.dto.NewStateResponseDto;
import se.digg.wallet.r2ps.commons.exception.ServiceRequestHandlingException;
import se.digg.wallet.r2ps.domain.model.EcPublicJwk;
import se.digg.wallet.r2ps.domain.model.HsmWorkerRequest;
import se.digg.wallet.r2ps.domain.model.R2psResponse;
import se.digg.wallet.r2ps.domain.model.StateInitRequest;
import se.digg.wallet.r2ps.domain.model.StateInitResponse;
import se.digg.wallet.r2ps.infrastructure.adapter.in.messaging.StateInitResponseCache;
import se.digg.wallet.r2ps.infrastructure.adapter.out.R2psDeviceStateValKey;
import se.digg.wallet.r2ps.infrastructure.config.Config;
import se.digg.wallet.r2ps.infrastructure.service.UrlFormatterService;

@RestController
public class R2psRequestController {

  private static final Logger log = LoggerFactory.getLogger(R2psRequestController.class);
  private final ObjectMapper objectMapper;
  private final R2psDeviceStateSpiPort r2psDeviceStateSpiPort;
  private final RequestMessageSpiPort requestMessageSpiPort;
  private final StateInitRequestSpiPort stateInitRequestSpiPort;
  private final StateInitResponseCache stateInitResponseCache;
  private final PendingRequestContextSpiPort pendingRequestContextSpiPort;
  private final R2psResponseUseCase r2psResponseUseCase;
  private final UrlFormatterService urlFormatter;

  private final boolean syncResponseSupport;
  private final long maxResponseTimeoutInMillis;

  public R2psRequestController(
      ObjectMapper objectMapper,
      final R2psDeviceStateSpiPort r2psDeviceStateSpiPort,
      final RequestMessageSpiPort requestMessageSpiPort,
      final StateInitRequestSpiPort stateInitRequestSpiPort,
      final StateInitResponseCache stateInitResponseCache,
      final PendingRequestContextSpiPort pendingRequestContextSpiPort,
      R2psResponseSinkSpiPort r2psResponseSinkSpiPort,
      UrlFormatterService urlFormatter,
      Config config) {
    this.objectMapper = objectMapper;
    this.r2psDeviceStateSpiPort = r2psDeviceStateSpiPort;
    this.requestMessageSpiPort = requestMessageSpiPort;
    this.stateInitRequestSpiPort = stateInitRequestSpiPort;
    this.stateInitResponseCache = stateInitResponseCache;
    this.pendingRequestContextSpiPort = pendingRequestContextSpiPort;
    this.urlFormatter = urlFormatter;
    syncResponseSupport = config.getKafka().rest().serveSync();
    maxResponseTimeoutInMillis = config.getKafka().rest().syncTimeoutMs();
    r2psResponseUseCase = new R2psResponseService(r2psResponseSinkSpiPort, r2psDeviceStateSpiPort,
        pendingRequestContextSpiPort);
  }

  @GetMapping("/task/{correlationId}")
  public ResponseEntity<AsyncResponseDto<String>> taskResponse(@PathVariable UUID correlationId) {

    Optional<R2psResponse> r2psResponse =
        r2psResponseUseCase.waitForR2psResponse(correlationId, maxResponseTimeoutInMillis);
    if (r2psResponse.isEmpty()) {
      URI location = urlFormatter.responseEventsUrl(correlationId);
      AsyncResponseDto<String> responseBody =
          new AsyncResponseDto<>(
              correlationId,
              AsyncResponseStatus.PENDING,
              Optional.empty(),
              Optional.of(location),
              Optional.empty());
      log.info("registerResponseDto pending {}", responseBody);
      return ResponseEntity.accepted().location(location).body(responseBody);
    }

    if (r2psResponse.get().httpStatus() != HttpStatus.OK.value()) {
      Optional<AsyncResponseError> errorPayload = parseErrorPayload(r2psResponse.get());

      AsyncResponseDto<String> registerResponseDto =
          new AsyncResponseDto<>(
              correlationId,
              AsyncResponseStatus.COMPLETE,
              Optional.empty(),
              Optional.empty(),
              errorPayload);
      return ResponseEntity.status(r2psResponse.get().httpStatus()).body(registerResponseDto);
    }

    AsyncResponseDto<String> registerResponseDto =
        new AsyncResponseDto<>(
            correlationId,
            AsyncResponseStatus.COMPLETE,
            Optional.of(r2psResponse.get().serviceResponseJws()),
            Optional.empty(),
            Optional.empty());
    log.info("registerResponseDto {}", registerResponseDto);

    return ResponseEntity.ok(registerResponseDto);
  }

  @PostMapping(
      value = "/",
      produces = MediaType.APPLICATION_JSON_VALUE,
      consumes = MediaType.APPLICATION_JSON_VALUE)
  public ResponseEntity<AsyncResponseDto<String>> service(@RequestBody final BffRequest bffRequest)
      throws Exception {
    if (log.isDebugEnabled()) {
      logServiceRequest(bffRequest.getOuterRequestJws());
    }

    UUID deviceId = UUID.fromString(bffRequest.getClientId());
    String stateJws = r2psDeviceStateSpiPort.load(deviceId.toString());

    if (stateJws == null) {
      log.info("No state found for deviceId: {}", deviceId);
      return ResponseEntity.status(HttpStatus.NOT_FOUND).build();
    }

    UUID requestId = UUID.randomUUID();
    pendingRequestContextSpiPort.save(requestId.toString(),
        new PendingRequestContext(deviceId.toString(), R2psDeviceStateValKey.DEFAULT_TTL_SECONDS));

    HsmWorkerRequest hsmWorkerRequest =
        new HsmWorkerRequest(requestId, stateJws, bffRequest.getOuterRequestJws());
    log.info("Sending service request:\n{}", objectMapper.writeValueAsString(hsmWorkerRequest));
    requestMessageSpiPort.send(hsmWorkerRequest, deviceId);

    if (syncResponseSupport) {
      log.info("Waiting for synchronous response for requestId: {}", requestId);
      return taskResponse(requestId);
    }

    URI location = urlFormatter.responseEventsUrl(requestId);
    AsyncResponseDto<String> responseBody =
        new AsyncResponseDto<>(
            requestId,
            AsyncResponseStatus.PENDING,
            Optional.empty(),
            Optional.of(location),
            Optional.empty());
    return ResponseEntity.accepted().location(location).body(responseBody);
  }

  @PostMapping(
      value = "/service",
      produces = MediaType.APPLICATION_JSON_VALUE,
      consumes = MediaType.APPLICATION_JSON_VALUE)
  public ResponseEntity<String> legacySyncService(@RequestBody final BffRequest serviceRequestJws)
      throws Exception {
    ResponseEntity<AsyncResponseDto<String>> serviceResponse = this.service(serviceRequestJws);
    if (serviceResponse.getBody() != null && serviceResponse.getBody().result().isPresent()) {
      String body = serviceResponse.getBody().result().get();
      log.info("Response {} {}", serviceResponse.getStatusCode(), body);
      return ResponseEntity.status(serviceResponse.getStatusCode()).body(body);
    }
    return ResponseEntity.status(HttpStatus.REQUEST_TIMEOUT).build();
  }

  /**
   * DEV-ONLY: overwrite and NewStateRequestDto.clientId must be removed before production.
   */
  @PostMapping(
      value = "/new_state",
      produces = MediaType.APPLICATION_JSON_VALUE,
      consumes = MediaType.APPLICATION_JSON_VALUE)
  public ResponseEntity<NewStateResponseDto> newState(@RequestBody NewStateRequestDto request)
      throws Exception {
    long ttlSeconds = parseTtl(request.ttl());

    // DEV-ONLY: allow caller to supply an existing clientId for overwrite; otherwise generate one
    String clientId = (request.clientId() != null && request.overwrite())
        ? request.clientId()
        : UUID.randomUUID().toString();

    if (!request.overwrite() && r2psDeviceStateSpiPort.load(clientId) != null) {
      return ResponseEntity.ok(new NewStateResponseDto("OK", clientId, null));
    }

    StateInitResponse initResponse = sendStateInitRequest(clientId, request.publicKey(), ttlSeconds);
    log.info("New state created for clientId: {}, dev_authorization_code: {}",
        clientId, initResponse.devAuthorizationCode());

    return ResponseEntity.ok(new NewStateResponseDto("OK", clientId, initResponse.devAuthorizationCode()));
  }

  private long parseTtl(String iso8601) {
    if (iso8601 == null) {
      return R2psDeviceStateValKey.DEFAULT_TTL_SECONDS;
    }
    return Duration.parse(iso8601).toSeconds();
  }

  private void logServiceRequest(final String serviceRequest) {
    log.trace("Service request JWS: {}", serviceRequest);
    try {
      JWSObject jwsObject = JWSObject.parse(serviceRequest);
      if (log.isTraceEnabled()) {
        log.trace(
            "Sending service request:\n{}",
            objectMapper.writeValueAsString(jwsObject.getPayload().toJSONObject()));
      }
    } catch (JsonProcessingException | ParseException e) {
      throw new RuntimeException(e);
    }
  }

  private Optional<AsyncResponseError> parseErrorPayload(R2psResponse r2psResponse) {
    // TODO this is not working since we're not sending error payloads on Kafka
    try {
      ServiceRequestHandlingException serviceRequestHandlingException =
          objectMapper.readValue(
              r2psResponse.serviceResponseJws(), ServiceRequestHandlingException.class);
      if (serviceRequestHandlingException != null) {
        log.info("Parsed error payload: {}", serviceRequestHandlingException.getMessage());
        return Optional.of(
            new AsyncResponseError(
                serviceRequestHandlingException.getMessage(),
                serviceRequestHandlingException.getErrorCode().getResponseCode()));
      }
    } catch (JsonProcessingException e) {
      log.debug("Could not parse error payload", e);
    }
    return Optional.empty();
  }

  private StateInitResponse sendStateInitRequest(String clientId, EcPublicJwk publicKey,
      long ttlSeconds) throws Exception {
    String requestId = UUID.randomUUID().toString();

    pendingRequestContextSpiPort.save(requestId, new PendingRequestContext(clientId, ttlSeconds));

    StateInitRequest request = new StateInitRequest(requestId, publicKey);
    stateInitRequestSpiPort.send(request, UUID.fromString(clientId));
    log.info("Sent state init request for clientId: {}, requestId: {}", clientId, requestId);

    return stateInitResponseCache.waitForResponse(requestId, Duration.ofSeconds(5))
        .orElseThrow(() -> new RuntimeException(
            "State initialization timeout for client: " + clientId));
  }
}
