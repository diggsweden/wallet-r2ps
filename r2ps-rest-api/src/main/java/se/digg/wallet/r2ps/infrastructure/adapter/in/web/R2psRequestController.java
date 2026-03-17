package se.digg.wallet.r2ps.infrastructure.adapter.in.web;

import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.nimbusds.jose.JWSObject;
import java.net.URI;
import java.text.ParseException;
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
import se.digg.wallet.r2ps.application.port.out.RequestMessageSpiPort;
import se.digg.wallet.r2ps.application.port.out.StateInitRequestSpiPort;
import se.digg.wallet.r2ps.commons.dto.BffRequest;
import se.digg.wallet.r2ps.commons.dto.NewStateRequestDto;
import se.digg.wallet.r2ps.commons.dto.NewStateResponseDto;
import se.digg.wallet.r2ps.domain.model.HsmWorkerRequest;
import se.digg.wallet.r2ps.domain.model.R2psResponse;
import se.digg.wallet.r2ps.domain.model.StateInitCommandDto;
import se.digg.wallet.r2ps.infrastructure.config.Config;
import se.digg.wallet.r2ps.infrastructure.service.UrlFormatterService;

@RestController
public class R2psRequestController {

  private static final Logger log = LoggerFactory.getLogger(R2psRequestController.class);
  private final ObjectMapper objectMapper;
  private final RequestMessageSpiPort requestMessageSpiPort;
  private final StateInitRequestSpiPort stateInitRequestSpiPort;
  private final PendingRequestContextSpiPort pendingRequestContextSpiPort;
  private final R2psResponseUseCase r2psResponseUseCase;
  private final UrlFormatterService urlFormatter;

  private final boolean syncResponseSupport;
  private final long maxResponseTimeoutInMillis;

  public R2psRequestController(
      ObjectMapper objectMapper,
      final RequestMessageSpiPort requestMessageSpiPort,
      final StateInitRequestSpiPort stateInitRequestSpiPort,
      final PendingRequestContextSpiPort pendingRequestContextSpiPort,
      R2psResponseUseCase r2psResponseUseCase,
      UrlFormatterService urlFormatter,
      Config config) {
    this.objectMapper = objectMapper;
    this.requestMessageSpiPort = requestMessageSpiPort;
    this.stateInitRequestSpiPort = stateInitRequestSpiPort;
    this.pendingRequestContextSpiPort = pendingRequestContextSpiPort;
    this.r2psResponseUseCase = r2psResponseUseCase;
    this.urlFormatter = urlFormatter;
    syncResponseSupport = config.getKafka().rest().serveSync();
    maxResponseTimeoutInMillis = config.getKafka().rest().syncTimeoutMs();
  }

  @GetMapping("/task/{correlationId}")
  public ResponseEntity<AsyncResponseDto<String>> taskResponse(
      @PathVariable String correlationId) {

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

    if (!"OK".equals(r2psResponse.get().status())) {
      Optional<AsyncResponseError> errorPayload = parseErrorPayload(r2psResponse.get());

      AsyncResponseDto<String> registerResponseDto =
          new AsyncResponseDto<>(
              correlationId,
              AsyncResponseStatus.COMPLETE,
              Optional.empty(),
              Optional.empty(),
              errorPayload);
      return ResponseEntity.status(HttpStatus.INTERNAL_SERVER_ERROR).body(registerResponseDto);
    }

    AsyncResponseDto<String> registerResponseDto =
        new AsyncResponseDto<>(
            correlationId,
            AsyncResponseStatus.COMPLETE,
            r2psResponse.get().outerResponseJws(),
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
    String correlationId = UUID.randomUUID().toString();

    // Save pending context mapping correlationId -> deviceId
    pendingRequestContextSpiPort.save(correlationId,
        new PendingRequestContext(deviceId.toString()));

    // Build command — no stateJws, state is server-owned
    HsmWorkerRequest hsmWorkerRequest =
        new HsmWorkerRequest(correlationId, deviceId.toString(), null, null,
            bffRequest.getOuterRequestJws());
    log.info("Sending service request: correlationId={}, deviceId={}", correlationId, deviceId);
    requestMessageSpiPort.send(hsmWorkerRequest, deviceId);

    if (syncResponseSupport) {
      log.info("Waiting for synchronous response for correlationId: {}", correlationId);
      return taskResponse(correlationId);
    }

    URI location = urlFormatter.responseEventsUrl(correlationId);
    AsyncResponseDto<String> responseBody =
        new AsyncResponseDto<>(
            correlationId,
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
   * Creates a new device state via the worker. Sends a StateInitCommandDto to r2ps-requests
   * and waits for the response via the standard Redis polling mechanism.
   *
   * DEV-ONLY: overwrite and NewStateRequestDto.clientId must be removed before production.
   */
  @PostMapping(
      value = "/new_state",
      produces = MediaType.APPLICATION_JSON_VALUE,
      consumes = MediaType.APPLICATION_JSON_VALUE)
  public ResponseEntity<NewStateResponseDto> newState(@RequestBody NewStateRequestDto request)
      throws Exception {

    // DEV-ONLY: allow caller to supply an existing clientId for overwrite; otherwise generate one
    String clientId = (request.clientId() != null && request.overwrite())
        ? request.clientId()
        : UUID.randomUUID().toString();

    String correlationId = UUID.randomUUID().toString();

    // Save pending context
    pendingRequestContextSpiPort.save(correlationId, new PendingRequestContext(clientId));

    // Build state-init command
    StateInitCommandDto command = new StateInitCommandDto(
        correlationId, clientId, "state-init", request.publicKey());

    stateInitRequestSpiPort.send(command, UUID.fromString(clientId));
    log.info("Sent state-init command for clientId={}, correlationId={}", clientId, correlationId);

    // Wait for response via Redis polling (same mechanism as service requests)
    Optional<R2psResponse> response =
        r2psResponseUseCase.waitForR2psResponse(correlationId, 5000);

    if (response.isEmpty()) {
      throw new RuntimeException("State initialization timeout for client: " + clientId);
    }

    R2psResponse r2psResponse = response.get();
    if (!"OK".equals(r2psResponse.status())) {
      log.error("State-init failed for clientId={}: {}", clientId, r2psResponse.errorMessage());
      return ResponseEntity.status(HttpStatus.INTERNAL_SERVER_ERROR)
          .body(new NewStateResponseDto("ERROR", clientId, null));
    }

    // Pass through the outer response JWS (contains JWE-encrypted InnerResponse with
    // dev_authorization_code, device_id, and hsm_state_version)
    String serviceResponseJws = r2psResponse.outerResponseJws().orElse(null);

    log.info("New state created for clientId={}", clientId);

    return ResponseEntity.ok(new NewStateResponseDto("OK", clientId, serviceResponseJws));
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
    return Optional.empty();
  }
}
