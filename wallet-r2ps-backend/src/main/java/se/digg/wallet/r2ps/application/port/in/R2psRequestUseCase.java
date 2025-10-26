package se.digg.wallet.r2ps.application.port.in;

import se.digg.wallet.r2ps.infrastructure.adapter.dto.R2psRequestDto;

public interface R2psRequestUseCase {
  void r2psRequest(R2psRequestDto r2psRequestDto);
}
