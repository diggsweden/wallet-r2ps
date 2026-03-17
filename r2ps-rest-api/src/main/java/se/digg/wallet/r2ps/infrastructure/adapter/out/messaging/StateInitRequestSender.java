package se.digg.wallet.r2ps.infrastructure.adapter.out.messaging;

import java.util.UUID;
import org.springframework.kafka.core.KafkaTemplate;
import org.springframework.stereotype.Service;
import se.digg.wallet.r2ps.application.port.out.StateInitRequestSpiPort;
import se.digg.wallet.r2ps.domain.model.StateInitCommandDto;

/**
 * Sends StateInitCommandDto to the r2ps-requests topic (same as regular commands).
 * The worker discriminates between command types by attempting deserialization.
 */
@Service
public class StateInitRequestSender implements StateInitRequestSpiPort {

  private static final String DEST_TOPIC = "r2ps-requests";

  private final KafkaTemplate<String, StateInitCommandDto> kafkaTemplate;

  public StateInitRequestSender(KafkaTemplate<String, StateInitCommandDto> kafkaTemplate) {
    this.kafkaTemplate = kafkaTemplate;
  }

  @Override
  public void send(StateInitCommandDto command, UUID deviceId) {
    kafkaTemplate.send(DEST_TOPIC, deviceId.toString(), command);
  }
}
