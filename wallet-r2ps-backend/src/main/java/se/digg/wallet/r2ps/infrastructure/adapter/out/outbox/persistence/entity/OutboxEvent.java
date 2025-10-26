package se.digg.wallet.r2ps.infrastructure.adapter.out.outbox.persistence.entity;

import jakarta.persistence.*;

import java.time.Instant;
import java.util.UUID;

@Entity
@Table(name = "outbox_events")
public class OutboxEvent {

  @Id
  private UUID id;

  @Column(nullable = false)
  private String aggregateId;

  @Column(nullable = false)
  private String aggregateType;

  @Column(nullable = false)
  private String eventType;

  @Column(nullable = false, columnDefinition = "TEXT")
  private String payload;

  @Column(nullable = false)
  private Instant createdAt;

  @Column
  private Instant publishedAt;

  @Column(nullable = false)
  private String status; // PENDING, PUBLISHED, FAILED

  @Column
  private Integer retryCount = 0;

  @Column
  private String correlationId;

  @Version
  private Long version;

  // Constructors
  public OutboxEvent() {}

  public OutboxEvent(
      UUID id,
      String aggregateId,
      String aggregateType,
      String eventType,
      String payload,
      String correlationId
  ) {
    this.id = id;
    this.aggregateId = aggregateId;
    this.aggregateType = aggregateType;
    this.eventType = eventType;
    this.payload = payload;
    this.correlationId = correlationId;
    this.createdAt = Instant.now();
    this.status = "PENDING";
    this.retryCount = 0;
  }

  public void markAsPublished() {
    this.status = "PUBLISHED";
    this.publishedAt = Instant.now();
  }

  public void markAsFailed() {
    this.status = "FAILED";
    this.retryCount++;
  }

  public void markForRetry() {
    this.retryCount++;
  }


  // Getters and setters
  public UUID getId() { return id; }
  public String getAggregateId() { return aggregateId; }
  public String getAggregateType() { return aggregateType; }
  public String getEventType() { return eventType; }
  public String getPayload() { return payload; }
  public Instant getCreatedAt() { return createdAt; }
  public Instant getPublishedAt() { return publishedAt; }
  public String getStatus() { return status; }
  public Integer getRetryCount() { return retryCount; }
  public String getCorrelationId() { return correlationId; }
  public Long getVersion() { return version; }
}
