package se.digg.wallet.r2ps.infrastructure.adapter.out.outbox.persistence;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.datatype.jdk8.Jdk8Module;
import org.springframework.stereotype.Component;
import org.springframework.transaction.annotation.Transactional;
import se.digg.wallet.r2ps.application.port.out.EventPublisherSpiPort;
import se.digg.wallet.r2ps.domain.event.DeviceKeyAdded;
import se.digg.wallet.r2ps.domain.event.DeviceKeyRevoked;
import se.digg.wallet.r2ps.domain.event.Event;
import se.digg.wallet.r2ps.domain.event.HsmKeyCreated;
import se.digg.wallet.r2ps.domain.event.HsmKeyDeleted;
import se.digg.wallet.r2ps.domain.event.ServerWalletRegistered;
import se.digg.wallet.r2ps.domain.event.ServerWalletRevoked;
import se.digg.wallet.r2ps.infrastructure.adapter.out.outbox.persistence.entity.OutboxEvent;

@Component
public class OutboxEventPublisher implements EventPublisherSpiPort {

  private final OutboxEventRepository outboxRepository;
  private final ObjectMapper objectMapper;

  public OutboxEventPublisher(
      OutboxEventRepository outboxRepository,
      ObjectMapper objectMapper
  ) {
    this.outboxRepository = outboxRepository;
    this.objectMapper = objectMapper;

    objectMapper.registerModule(new Jdk8Module());
  }

  @Override
  @Transactional
  public void publish(Event event) {
    try {
      String payload = objectMapper.writeValueAsString(event);

      OutboxEvent outboxEvent = new OutboxEvent(
          java.util.UUID.randomUUID(),
          extractAggregateId(event),
          extractAggregateType(event),
          event.getClass().getSimpleName(),
          payload,
          extractCorrelationId(event)
      );

      outboxRepository.save(outboxEvent);
    } catch (Exception e) {
      throw new RuntimeException("Failed to save event to outbox", e);
    }
  }

  private String extractAggregateId(Event event) {
    // Extract based on event type - default is walletId
    return event.metadata().walletId().toString();
  }

  private String extractAggregateType(Event event) {
    // TODO bygga in i metadata?
    return switch (event) {
      case ServerWalletRegistered c -> "ServerWalletRegistered";
      case ServerWalletRevoked c -> "ServerWalletRevoked";
      case DeviceKeyAdded c -> "DeviceKeyAdded";
      case DeviceKeyRevoked c -> "DeviceKeyRevoked";
      case HsmKeyCreated c -> "HsmKeyCreated";
      case HsmKeyDeleted c -> "HsmKeyDeleted";
      default -> "Unknown";
    };
  }

  private String extractCorrelationId(Event event) {
    if (event.metadata().correlationId().isPresent()) {
      return event.metadata().correlationId().get().toString();
    }
    return null;
  }
}
