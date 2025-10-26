package se.digg.wallet.r2ps.infrastructure.adapter.in.web;

import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RestController;
import se.digg.wallet.r2ps.infrastructure.adapter.dto.R2psResponseDto;
import se.digg.wallet.r2ps.application.port.in.R2psRequestUseCase;
import se.digg.wallet.r2ps.domain.model.R2psRequest;
import se.digg.wallet.r2ps.domain.model.R2psResponse;
import se.digg.wallet.r2ps.infrastructure.adapter.dto.R2psResponseDtoBuilder;

import java.util.List;
import java.util.UUID;

@RestController
public class R2psRequestController {
  private final R2psRequestUseCase r2psRequestUseCase;

  public R2psRequestController(R2psRequestUseCase r2psRequestUseCase) {
    this.r2psRequestUseCase = r2psRequestUseCase;
  }

  @PostMapping("/r2ps")
  public R2psResponseDto handleR2psRequest(@RequestBody String payload) {

    R2psRequest r2psRequest = new R2psRequest(payload);
    R2psResponse r2psResponse = r2psRequestUseCase.r2psRequest(r2psRequest);

    return R2psResponseDtoBuilder.builder()
        .requestId(UUID.randomUUID()) // TODO
        .deviceId("deviceId")// TODO
        .httpStatus(r2psResponse.httpStatus())
        .payload(r2psResponse.payload())
        .pakeSessionId("pakeSessionId")// TODO
        .events(List.of())// TODO
        .build();
  }

}
