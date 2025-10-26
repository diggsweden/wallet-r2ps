package se.digg.wallet.r2ps.infrastructure.adapter.in.messaging;

import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.databind.ObjectMapper;
import org.apache.kafka.clients.consumer.ConsumerRecord;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.data.redis.core.RedisTemplate;
import org.springframework.kafka.annotation.KafkaListener;
import org.springframework.stereotype.Service;
import se.digg.wallet.r2ps.infrastructure.adapter.dto.R2psResponseDto;

import static java.lang.Thread.sleep;

@Service
public class R2psResponseSink {
/** temporary work-around for synchronous client  **/
  private static final String SOURCE_TOPIC = "r2ps-responses";
  private static final Logger logger = LoggerFactory.getLogger(R2psResponseSink.class);

  private final ObjectMapper objectMapper;
  private final RedisTemplate<String, R2psResponseDto> redisTemplate;

  public R2psResponseSink(ObjectMapper objectMapper, RedisTemplate<String, R2psResponseDto> redisTemplate) {
    this.objectMapper = objectMapper;
    this.redisTemplate = redisTemplate;
  }


  public R2psResponseDto waitForSecureChannelResponse(String key, long timeoutSeconds)
      throws InterruptedException {
    long endTime = System.currentTimeMillis() + (timeoutSeconds * 1000);

    while (System.currentTimeMillis() < endTime) {
      if (Boolean.TRUE.equals(redisTemplate.hasKey(key))) {
        return redisTemplate.opsForValue().get(key);
      }
      sleep(100); // Poll every 100ms
    }

    return null; // Timeout
  }

  @KafkaListener(topics = SOURCE_TOPIC, groupId = "r2ps-response-sink")
  public void consume(ConsumerRecord<String, String> record) {
    // TODO deserialize directly to domain model in the listener
    String key = record.key();
    R2psResponseDto r2psResponseDto = null;
    try {
      r2psResponseDto = objectMapper.readValue(record.value(), R2psResponseDto.class);
    } catch (JsonProcessingException e) {
      logger.error("Could not deserialize message {} ", record.value(), e);
      return;
    }

    logger.info("Received message - Key: {}, payload: {}",
        key, r2psResponseDto);

    redisTemplate.opsForValue().set(r2psResponseDto.requestId().toString(), r2psResponseDto);
    // TODO ttl for key


  }
}
