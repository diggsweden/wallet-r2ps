package se.digg.wallet.r2ps.infrastructure.adapter.in.messaging;

import com.fasterxml.jackson.core.JsonProcessingException;
import java.util.Optional;
import com.fasterxml.jackson.databind.ObjectMapper;
import org.apache.kafka.clients.consumer.ConsumerRecord;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.kafka.annotation.KafkaListener;
import org.springframework.stereotype.Service;
import se.digg.wallet.r2ps.application.dto.PendingRequestContext;
import se.digg.wallet.r2ps.application.port.out.PendingRequestContextSpiPort;
import se.digg.wallet.r2ps.application.port.out.R2psDeviceStateSpiPort;
import se.digg.wallet.r2ps.domain.model.StateInitResponse;

@Service
public class StateInitResponseReceiver {

  private static final Logger log = LoggerFactory.getLogger(StateInitResponseReceiver.class);

  private final ObjectMapper objectMapper;
  private final R2psDeviceStateSpiPort deviceStateSpiPort;
  private final PendingRequestContextSpiPort pendingRequestContextSpiPort;
  private final StateInitResponseCache responseCache;

  public StateInitResponseReceiver(
      ObjectMapper objectMapper,
      R2psDeviceStateSpiPort deviceStateSpiPort,
      PendingRequestContextSpiPort pendingRequestContextSpiPort,
      StateInitResponseCache responseCache) {
    this.objectMapper = objectMapper;
    this.deviceStateSpiPort = deviceStateSpiPort;
    this.pendingRequestContextSpiPort = pendingRequestContextSpiPort;
    this.responseCache = responseCache;
  }

  @KafkaListener(topics = "state-init-responses", groupId = "${r2ps.in.group-id}")
  public void consume(ConsumerRecord<String, String> record) {
    String key = record.key();
    StateInitResponse response = null;

    try {
      response = objectMapper.readValue(record.value(), StateInitResponse.class);
    } catch (JsonProcessingException e) {
      log.error("Could not deserialize state init response: {}", record.value(), e);
      return;
    }

    log.info("Received state init response - Key: {}, requestId: {}", key, response.requestId());

    String requestId = response.requestId();
    Optional<PendingRequestContext> ctxOpt = pendingRequestContextSpiPort.load(requestId);
    if (ctxOpt.isEmpty()) {
      log.warn("No pending context for state-init requestId: {}, ignoring", requestId);
      return;
    }
    PendingRequestContext ctx = ctxOpt.get();

    deviceStateSpiPort.save(ctx.stateKey(), response.stateJws(), ctx.ttlSeconds());
    log.debug("Saved state to Redis for clientId: {}", ctx.stateKey());

    responseCache.put(response.requestId(), response);
  }
}
