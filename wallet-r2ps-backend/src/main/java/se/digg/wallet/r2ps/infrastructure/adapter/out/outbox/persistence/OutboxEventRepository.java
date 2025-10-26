package se.digg.wallet.r2ps.infrastructure.adapter.out.outbox.persistence;

import org.springframework.data.jpa.repository.JpaRepository;
import org.springframework.data.jpa.repository.Query;
import se.digg.wallet.r2ps.infrastructure.adapter.out.outbox.persistence.entity.OutboxEvent;

import java.util.List;
import java.util.UUID;

public interface OutboxEventRepository extends JpaRepository<OutboxEvent, UUID> {

  @Query("SELECT e FROM OutboxEvent e WHERE e.status = 'PENDING' ORDER BY e.createdAt ASC")
  List<OutboxEvent> findPendingEvents();

  @Query(
      "SELECT e FROM OutboxEvent e WHERE e.status = 'FAILED' AND e.retryCount < 5 ORDER BY e.createdAt ASC")
  default List<OutboxEvent> findFailedEventsForRetry() {
    return null;
  }
}
