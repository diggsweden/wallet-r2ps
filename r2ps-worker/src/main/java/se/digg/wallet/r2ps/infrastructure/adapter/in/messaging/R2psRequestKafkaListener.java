package se.digg.wallet.r2ps.infrastructure.adapter.in.messaging;

import com.fasterxml.jackson.core.JsonProcessingException;
import com.fasterxml.jackson.databind.ObjectMapper;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.kafka.annotation.KafkaListener;
import org.springframework.stereotype.Component;
import se.digg.wallet.r2ps.application.port.out.R2psResponseSpiPort;
import se.digg.wallet.r2ps.domain.domain.model.R2psResponse;
import se.digg.wallet.r2ps.domain.domain.model.event.Event;
import se.digg.wallet.r2ps.infrastructure.adapter.dto.R2psRequestDto;
import se.digg.wallet.r2ps.application.port.in.R2psRequestUseCase;
import se.digg.wallet.r2ps.domain.domain.model.R2psRequest;
import se.digg.wallet.r2ps.infrastructure.config.Config;

import java.util.List;
import java.util.UUID;

@Component
public class R2psRequestKafkaListener {


  private static final Logger log = LoggerFactory.getLogger(R2psRequestKafkaListener.class);

  private final R2psRequestUseCase r2psRequestUseCase;
  private final R2psResponseSpiPort r2psResponseSpiPort;
  private final ObjectMapper objectMapper;

  public R2psRequestKafkaListener(R2psRequestUseCase r2psRequestUseCase, Config config,
      R2psResponseSpiPort r2psResponseSpiPort,
      ObjectMapper objectMapper) {
    this.r2psRequestUseCase = r2psRequestUseCase;
    this.r2psResponseSpiPort = r2psResponseSpiPort;
    this.objectMapper = objectMapper;
  }

  @KafkaListener(topics = "${r2ps.worker.in.topic}", groupId = "${r2ps.worker.in.group-id}")
  public void consumeR2psRequest(String message) {
    // TODO deserialize directly to domain model in the listener
    R2psRequestDto r2psRequestDto = null;
    try {
      r2psRequestDto = objectMapper.readValue(message, R2psRequestDto.class);
      R2psResponse r2psResponse =
          r2psRequestUseCase.r2psRequest(new R2psRequest(r2psRequestDto.requestId(),
              r2psRequestDto.walletId(), r2psRequestDto.deviceId(), r2psRequestDto.payload()));
      r2psResponseSpiPort.r2psResponse(r2psResponse);


    } catch (JsonProcessingException e) {
      log.error("Could not deserialize message {} ", message, e);
    }
  }
}
