package se.digg.wallet.r2ps.infrastructure.adapter.out.messaging;

import org.springframework.kafka.core.KafkaTemplate;
import org.springframework.stereotype.Service;
import se.digg.wallet.r2ps.application.dto.AsyncResponseDto;
import se.digg.wallet.r2ps.application.dto.AsyncResponseStatus;
import se.digg.wallet.r2ps.application.port.out.R2psRequestMessageSpiPort;
import se.digg.wallet.r2ps.infrastructure.adapter.dto.R2psRequestDto;
import se.digg.wallet.r2ps.infrastructure.adapter.dto.R2psResponseDto;
import se.digg.wallet.r2ps.infrastructure.service.UrlFormatterService;

import java.util.Optional;

@Service
public class R2psRequestMessageSender implements R2psRequestMessageSpiPort {

  private static final String DEST_TOPIC = "r2ps-requests";

  private final KafkaTemplate<String, R2psRequestDto> kafkaTemplate;

  public R2psRequestMessageSender(KafkaTemplate<String, R2psRequestDto> kafkaTemplate) {
    this.kafkaTemplate = kafkaTemplate;
  }

  @Override
  public void sendR2psRequestMessage(R2psRequestDto r2psRequestDto) {
    kafkaTemplate.send(DEST_TOPIC, r2psRequestDto.walletId().toString(), r2psRequestDto);
  }

}
