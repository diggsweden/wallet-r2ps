package se.digg.wallet.r2ps.infrastructure.adapter.out.messaging;

import org.springframework.kafka.core.KafkaTemplate;
import org.springframework.stereotype.Service;
import se.digg.wallet.r2ps.infrastructure.adapter.dto.R2psResponseDto;
import se.digg.wallet.r2ps.application.port.out.R2psResponseSpiPort;
import se.digg.wallet.r2ps.domain.model.R2psResponse;
import se.digg.wallet.r2ps.infrastructure.adapter.dto.R2psResponseDtoBuilder;

import java.util.List;
import java.util.UUID;

@Service
public class R2psResponseKafkaPublisher implements R2psResponseSpiPort {

  private final KafkaTemplate<String, R2psResponseDto> kafkaTemplate;

  public R2psResponseKafkaPublisher(KafkaTemplate<String, R2psResponseDto> kafkaTemplate) {
    this.kafkaTemplate = kafkaTemplate;
  }

  @Override
  public void r2psResponse(R2psResponse r2psResponse) {
    R2psResponseDto r2psResponseDto = R2psResponseDtoBuilder.builder()
        .requestId(UUID.randomUUID()) // TODO
        .deviceId("deviceId")// TODO
        .httpStatus(r2psResponse.httpStatus())
        .payload(r2psResponse.payload())
        .pakeSessionId("pakeSessionId")// TODO
        .events(List.of())// TODO
        .build();
    kafkaTemplate.send("r2ps-responses", r2psResponseDto);
  }

}
