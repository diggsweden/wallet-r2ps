package se.digg.wallet.r2ps.application.port.out;

import se.digg.wallet.r2ps.infrastructure.adapter.dto.R2psRequestDto;

public interface R2psRequestMessageSpiPort {
    void sendR2psRequestMessage(R2psRequestDto r2psRequestDto);
}
