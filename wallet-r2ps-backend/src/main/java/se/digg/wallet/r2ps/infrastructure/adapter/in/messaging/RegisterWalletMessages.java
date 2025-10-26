package se.digg.wallet.r2ps.infrastructure.adapter.in.messaging;

import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.databind.ObjectMapper;
import org.apache.kafka.clients.consumer.ConsumerRecord;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.data.redis.core.RedisTemplate;
import org.springframework.kafka.annotation.KafkaListener;
import org.springframework.stereotype.Service;
import se.digg.wallet.r2ps.application.dto.command.Command;
import se.digg.wallet.r2ps.infrastructure.adapter.dto.R2psResponseDto;

import static java.lang.Thread.sleep;

@Service
public class RegisterWalletMessages {

  private static final String SOURCE_TOPIC = "wallet-commands";
  private static final Logger logger = LoggerFactory.getLogger(RegisterWalletMessages.class);

  private final ObjectMapper objectMapper;

  public RegisterWalletMessages(ObjectMapper objectMapper) {
    this.objectMapper = objectMapper;
  }

  @KafkaListener(topics = SOURCE_TOPIC, groupId = "command-worker-group")
  public void consume(ConsumerRecord<String, String> record) {
    // TODO deserialize directly to domain model in the listener
    String key = record.key();
    Command command = null;
    try {
      command = objectMapper.readValue(record.value(), Command.class);
    } catch (JsonProcessingException e) {
      logger.error("Could not deserialize message {} ", record.value(), e);
      return;
    }

    logger.info("Received message - Key: {}, payload: {}",
        key, command);


    // TODO ttl for key


  }
}
