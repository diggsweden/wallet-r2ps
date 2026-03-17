package se.digg.wallet.r2ps.application.port.out;

import java.util.UUID;
import se.digg.wallet.r2ps.domain.model.StateInitCommandDto;

public interface StateInitRequestSpiPort {
  void send(StateInitCommandDto command, UUID deviceId);
}
