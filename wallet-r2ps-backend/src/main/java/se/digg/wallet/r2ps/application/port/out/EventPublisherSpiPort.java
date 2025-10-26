package se.digg.wallet.r2ps.application.port.out;

import se.digg.wallet.r2ps.domain.event.Event;


public interface EventPublisherSpiPort {
  void publish(Event event);
}
