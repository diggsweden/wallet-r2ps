package se.digg.wallet.r2ps.infrastructure.service;

import org.springframework.stereotype.Service;
import se.digg.wallet.r2ps.application.dto.AsyncResponseDto;
import se.digg.wallet.r2ps.infrastructure.config.Config;

import java.net.URI;
import java.util.UUID;

@Service
public class UrlFormatterService {
  private final Config config;

  public UrlFormatterService(Config config) {
    this.config = config;
  }

  public URI responseUrl(AsyncResponseDto<?> asyncResponseDto) {
    return responseUrl(asyncResponseDto.correlationId());
  }

  public URI responseUrl(UUID correlationId) {
    return URI.create(
        String.format(config.getKafka().rest().responseTemplateUrl(), correlationId.toString()));
  }
}
