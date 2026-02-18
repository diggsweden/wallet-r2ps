package se.digg.wallet.r2ps.infrastructure.adapter.out;

import org.springframework.data.redis.core.RedisTemplate;
import org.springframework.stereotype.Service;
import se.digg.wallet.r2ps.application.dto.PendingRequestContext;
import se.digg.wallet.r2ps.application.port.out.PendingRequestContextSpiPort;

import java.util.Optional;
import java.util.concurrent.TimeUnit;

@Service
public class PendingRequestContextValKey implements PendingRequestContextSpiPort {

  private static final long PENDING_TTL_SECONDS = 120;
  private static final String KEY_PREFIX = "pending-ctx:";

  private final RedisTemplate<String, PendingRequestContext> redisTemplate;

  public PendingRequestContextValKey(RedisTemplate<String, PendingRequestContext> redisTemplate) {
    this.redisTemplate = redisTemplate;
  }

  @Override
  public void save(String requestId, PendingRequestContext ctx) {
    redisTemplate.opsForValue().set(KEY_PREFIX + requestId, ctx, PENDING_TTL_SECONDS, TimeUnit.SECONDS);
  }

  @Override
  public Optional<PendingRequestContext> load(String requestId) {
    return Optional.ofNullable(redisTemplate.opsForValue().get(KEY_PREFIX + requestId));
  }
}
