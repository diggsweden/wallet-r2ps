package se.digg.wallet.r2ps.infrastructure.adapter.in.messaging;

import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.databind.ObjectMapper;
import org.apache.kafka.clients.consumer.ConsumerRecord;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.data.redis.core.RedisTemplate;
import org.springframework.kafka.annotation.KafkaListener;
import org.springframework.stereotype.Service;
import se.digg.wallet.r2ps.application.port.in.R2psResponseUseCase;
import se.digg.wallet.r2ps.application.port.out.R2psResponseSinkSpiPort;
import se.digg.wallet.r2ps.application.service.R2psResponseService;
import se.digg.wallet.r2ps.domain.model.R2psResponse;
import se.digg.wallet.r2ps.infrastructure.adapter.dto.R2psResponseDto;
import se.digg.wallet.r2ps.infrastructure.config.Config;

import static java.lang.Thread.sleep;

@Service
public class R2psResponseReadyMessageReceiver {

  /** temporary work-around for synchronous client **/
  private static final String SOURCE_TOPIC = "r2ps-responses";
  private static final long RESPONSE_TTL_SECONDS = 120;

  private static final Logger log = LoggerFactory.getLogger(R2psResponseReadyMessageReceiver.class);

  private final ObjectMapper objectMapper;
  private final R2psResponseUseCase r2psResponseUseCase;

  public R2psResponseReadyMessageReceiver(ObjectMapper objectMapper,
      R2psResponseSinkSpiPort r2psResponseSinkSpiPort) {
    this.objectMapper = objectMapper;
    r2psResponseUseCase = new R2psResponseService(r2psResponseSinkSpiPort);
  }


  @KafkaListener(topics = "${r2ps.in.topic}", groupId = "${r2ps.in.group-id}")
  public void consume(ConsumerRecord<String, String> record) {
    // TODO deserialize directly to domain model in the listener
    String key = record.key();
    R2psResponse r2psResponse = null;
    try {
      r2psResponse = objectMapper.readValue(record.value(), R2psResponse.class);
    } catch (JsonProcessingException e) {
      log.error("Could not deserialize message {} ", record.value(), e);
      return;
    }

    log.info("Received message - Key: {}, payload: {}",
        key, r2psResponse);

    r2psResponseUseCase.r2psResponseReady(r2psResponse);

  }
}
