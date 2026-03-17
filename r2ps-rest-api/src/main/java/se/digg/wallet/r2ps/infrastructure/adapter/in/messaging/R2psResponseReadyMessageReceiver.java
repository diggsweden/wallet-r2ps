package se.digg.wallet.r2ps.infrastructure.adapter.in.messaging;

import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.databind.ObjectMapper;
import org.apache.kafka.clients.consumer.ConsumerRecord;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.kafka.annotation.KafkaListener;
import org.springframework.stereotype.Service;
import se.digg.wallet.r2ps.application.port.in.R2psResponseUseCase;
import se.digg.wallet.r2ps.domain.model.R2psResponse;

@Service
public class R2psResponseReadyMessageReceiver {

  private static final Logger log = LoggerFactory.getLogger(R2psResponseReadyMessageReceiver.class);

  private final ObjectMapper objectMapper;
  private final R2psResponseUseCase r2psResponseUseCase;

  public R2psResponseReadyMessageReceiver(ObjectMapper objectMapper,
      R2psResponseUseCase r2psResponseUseCase) {
    this.objectMapper = objectMapper;
    this.r2psResponseUseCase = r2psResponseUseCase;
  }

  @KafkaListener(topics = "${r2ps.in.topic}", groupId = "${r2ps.in.group-id}")
  public void consume(ConsumerRecord<String, String> record) {
    String key = record.key();
    R2psResponse r2psResponse;
    try {
      r2psResponse = objectMapper.readValue(record.value(), R2psResponse.class);
    } catch (JsonProcessingException e) {
      log.error("Could not deserialize message {} ", record.value(), e);
      return;
    }

    log.info("Received response - Key: {}, correlationId: {}, status: {}",
        key, r2psResponse.correlationId(), r2psResponse.status());

    r2psResponseUseCase.r2psResponseReady(r2psResponse);
  }
}
