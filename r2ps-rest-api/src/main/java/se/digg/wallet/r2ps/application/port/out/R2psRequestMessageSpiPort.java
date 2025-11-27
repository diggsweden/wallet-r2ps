package se.digg.wallet.r2ps.application.port.out;

import se.digg.wallet.r2ps.application.dto.AsyncResponseDto;
import se.digg.wallet.r2ps.domain.model.R2psRequest;
import se.digg.wallet.r2ps.infrastructure.adapter.dto.R2psRequestDto;
import se.digg.wallet.r2ps.infrastructure.adapter.dto.R2psResponseDto;

public interface R2psRequestMessageSpiPort {
  void sendR2psRequestMessage(R2psRequest r2psRequest);
}
