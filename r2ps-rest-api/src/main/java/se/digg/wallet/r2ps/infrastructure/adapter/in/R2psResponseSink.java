package se.digg.wallet.r2ps.infrastructure.adapter.in;

import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.databind.ObjectMapper;
import org.apache.kafka.clients.consumer.ConsumerRecord;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.data.redis.core.RedisTemplate;
import org.springframework.kafka.annotation.KafkaListener;
import org.springframework.stereotype.Service;
import se.digg.wallet.r2ps.infrastructure.adapter.dto.R2psResponseDto;
import se.digg.wallet.r2ps.infrastructure.config.Config;

import java.util.Optional;
import java.util.UUID;
import java.util.concurrent.TimeUnit;

import static java.lang.Thread.sleep;

@Service
public class R2psResponseSink {

  /** temporary work-around for synchronous client **/
  private static final String SOURCE_TOPIC = "r2ps-responses";
  private static final long RESPONSE_TTL_SECONDS = 120;

  private static final Logger log = LoggerFactory.getLogger(R2psResponseSink.class);

  private final Config config;
  private final ObjectMapper objectMapper;
  private final RedisTemplate<String, R2psResponseDto> redisTemplate;

  public R2psResponseSink(Config config, ObjectMapper objectMapper,
      RedisTemplate<String, R2psResponseDto> redisTemplate) {
    this.config = config;
    this.objectMapper = objectMapper;
    this.redisTemplate = redisTemplate;
  }


  public Optional<R2psResponseDto> waitForR2psResponse(UUID correlationId, long timeoutMillis) {
    long endTime = System.currentTimeMillis() + timeoutMillis;

    try {
      while (System.currentTimeMillis() < endTime) {
        R2psResponseDto r2psResponseDto = redisTemplate.opsForValue().get(correlationId.toString());
        if (r2psResponseDto != null) {
          log.info("Got r2psResponseDto for {}", correlationId);
          return Optional.of(r2psResponseDto);
        }
        sleep(100); // poll interval
      }
    } catch (InterruptedException e) {
      log.info("Interrupted while waiting for register wallet response for correlationId: {}",
          correlationId);
    }
    return Optional.empty();
  }

  @KafkaListener(topics = "${r2ps.in.topic}", groupId = "${r2ps.in.group-id}")
  public void consume(ConsumerRecord<String, String> record) {
    // TODO deserialize directly to domain model in the listener
    String key = record.key();
    R2psResponseDto r2psResponseDto = null;
    try {
      r2psResponseDto = objectMapper.readValue(record.value(), R2psResponseDto.class);
    } catch (JsonProcessingException e) {
      log.error("Could not deserialize message {} ", record.value(), e);
      return;
    }

    log.info("Received message - Key: {}, payload: {}",
        key, r2psResponseDto);

    redisTemplate.opsForValue().set(
        r2psResponseDto.requestId().toString(), r2psResponseDto, RESPONSE_TTL_SECONDS,
        TimeUnit.SECONDS);

  }
}
