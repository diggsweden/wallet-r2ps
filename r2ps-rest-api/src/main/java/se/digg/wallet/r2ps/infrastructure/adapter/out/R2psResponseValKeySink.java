package se.digg.wallet.r2ps.infrastructure.adapter.out;

import com.fasterxml.jackson.databind.ObjectMapper;
import org.springframework.data.redis.core.RedisTemplate;
import org.springframework.stereotype.Service;
import se.digg.wallet.r2ps.application.port.out.R2psResponseSinkSpiPort;
import se.digg.wallet.r2ps.domain.model.R2psResponse;
import se.digg.wallet.r2ps.infrastructure.config.Config;

import java.util.Optional;
import java.util.UUID;
import java.util.concurrent.TimeUnit;

@Service
public class R2psResponseValKeySink implements R2psResponseSinkSpiPort {

  private final Config config;
  private final RedisTemplate<String, R2psResponse> redisTemplate;

  public R2psResponseValKeySink(Config config, ObjectMapper objectMapper,
      RedisTemplate<String, R2psResponse> redisTemplate) {
    this.config = config;
    this.redisTemplate = redisTemplate;
  }

  @Override
  public void storeResponse(R2psResponse r2psResponse) {
    redisTemplate.opsForValue().set(r2psResponse.requestId().toString(), r2psResponse,
        config.getKafka().rest()
            .responseTtlSeconds(),
        TimeUnit.SECONDS);
  }

  @Override
  public Optional<R2psResponse> loadResponse(UUID requestId) {
    return Optional.ofNullable(redisTemplate.opsForValue().get(requestId.toString()));
  }
}
