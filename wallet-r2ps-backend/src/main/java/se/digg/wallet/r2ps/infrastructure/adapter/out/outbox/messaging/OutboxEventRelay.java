package se.digg.wallet.r2ps.infrastructure.adapter.out.outbox.messaging;

import com.fasterxml.jackson.databind.ObjectMapper;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.kafka.core.KafkaTemplate;
import org.springframework.scheduling.annotation.Scheduled;
import org.springframework.stereotype.Service;
import org.springframework.transaction.annotation.Transactional;
import se.digg.wallet.r2ps.domain.event.Event;
import se.digg.wallet.r2ps.infrastructure.adapter.out.outbox.persistence.OutboxEventRepository;
import se.digg.wallet.r2ps.infrastructure.adapter.out.outbox.persistence.entity.OutboxEvent;

import java.util.List;

@Service
public class OutboxEventRelay {
 // TODO: this implementation will not work. Have to be resilient regarding kafka failures etc.

  private static final Logger log = LoggerFactory.getLogger(OutboxEventRelay.class);
  private static final String EVENTS_TOPIC = "wallet-events";
  private static final String EVENTS_DLQ_TOPIC = "wallet-events-dlq";
  private final ObjectMapper objectMapper;

  private final OutboxEventRepository outboxRepository;
  private final KafkaTemplate<String, Event> kafkaTemplate;
  private final KafkaTemplate<String, String> kafkaTemplateString;

  public OutboxEventRelay(
      ObjectMapper objectMapper, OutboxEventRepository outboxRepository,
      KafkaTemplate<String, Event> kafkaTemplate, KafkaTemplate<String, String> kafkaTemplateString

  ) {
    this.objectMapper = objectMapper;
    this.outboxRepository = outboxRepository;
    this.kafkaTemplate = kafkaTemplate;
    this.kafkaTemplateString = kafkaTemplateString;
  }

  @Scheduled(fixedDelay = 5000) // Run every 5 seconds
  @Transactional
  public void relayPendingEvents() {
    log.info("Poll pending events to relay");

    List<OutboxEvent> pendingEvents = outboxRepository.findPendingEvents();

    log.info("Found {} pending events to relay", pendingEvents.size());


    for (OutboxEvent event : pendingEvents) {
      try {
        Event payload = objectMapper.readValue(event.getPayload(), Event.class);
        publishToKafka(payload);
        event.markAsPublished();
        outboxRepository.save(event);
        log.info("Successfully published event {} to Kafka", event.getId());
      } catch (Exception e) {
        log.error("Failed to publish event {} to Kafka", event.getId(), e);

        if (event.getRetryCount()>3) {
          event.markAsFailed();
        }
        else {
          event.markForRetry();
        }

        outboxRepository.save(event);
        return;
      }
    }
  }

  @Scheduled(fixedDelay = 60000) // Retry every 60 seconds
  @Transactional
  public void relayFailedEventsToDlq() {
    List<OutboxEvent> failedEvents = outboxRepository.findFailedEventsForRetry();
    if (failedEvents == null) return;
    log.debug("Found {} failed events to retry", failedEvents.size());

    for (OutboxEvent event : failedEvents) {
      try {
        publishToDlq(event);
        event.markAsPublished();
        outboxRepository.save(event);
        log.info("Successfully retried event {}", event.getId());
      } catch (Exception e) {
        log.error("Failed to retry event {}", event.getId(), e);
        event.markAsFailed();
        outboxRepository.save(event);
      }
    }
  }

  private void publishToKafka(Event event) {
    kafkaTemplate.send(
        EVENTS_TOPIC,
        event.metadata().walletId().toString(),
        event
    ).whenComplete((result, ex) -> {
      if (ex != null) {
        throw new RuntimeException("Kafka send failed", ex);
      }
    });
  }

  private void publishToDlq(OutboxEvent event) {
    kafkaTemplateString.send(
        EVENTS_DLQ_TOPIC,
        event.getAggregateId(),
        event.getPayload()
    ).whenComplete((result, ex) -> {
      if (ex != null) {
        throw new RuntimeException("Kafka send failed", ex);
      }
    });
  }
}
