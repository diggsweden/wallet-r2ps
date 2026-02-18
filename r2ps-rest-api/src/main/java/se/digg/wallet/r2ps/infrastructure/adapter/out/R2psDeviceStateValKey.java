package se.digg.wallet.r2ps.infrastructure.adapter.out;

import org.springframework.data.redis.core.RedisTemplate;
import org.springframework.stereotype.Service;
import se.digg.wallet.r2ps.application.port.out.R2psDeviceStateSpiPort;

import java.time.Duration;
import java.util.concurrent.TimeUnit;

@Service
public class R2psDeviceStateValKey implements R2psDeviceStateSpiPort {

  public static final long DEFAULT_TTL_SECONDS = Duration.ofDays(30).toSeconds();

  private final RedisTemplate<String, String> redisTemplate;

  public R2psDeviceStateValKey(RedisTemplate<String, String> redisTemplate) {
    this.redisTemplate = redisTemplate;
  }

  @Override
  public void save(String deviceId, String state, long ttlSeconds) {
    redisTemplate.opsForValue().set(deviceId, state, ttlSeconds, TimeUnit.SECONDS);
  }

  @Override
  public String load(String deviceId) {
    return redisTemplate.opsForValue().get(deviceId);
  }

}
