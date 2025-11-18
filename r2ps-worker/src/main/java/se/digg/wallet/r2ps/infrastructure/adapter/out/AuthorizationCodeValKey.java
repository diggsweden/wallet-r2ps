package se.digg.wallet.r2ps.infrastructure.adapter.out;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.data.redis.core.RedisTemplate;
import se.digg.wallet.r2ps.application.port.out.AuthorizationCodeSpiPort;

import java.time.Duration;
import java.time.temporal.TemporalUnit;
import java.util.Optional;
import java.util.UUID;

public class AuthorizationCodeValKey implements AuthorizationCodeSpiPort {

  private static final Logger log = LoggerFactory.getLogger(AuthorizationCodeValKey.class);
  private final RedisTemplate<String, String> redisTemplate;

  public AuthorizationCodeValKey(RedisTemplate<String, String> redisTemplate) {
    this.redisTemplate = redisTemplate;
  }

  @Override
  public void setAuthorizationCode(UUID walletId, String kid, String authorizationCode) {
    log.info("Storing authorization code for walletId: {}, kid: {} code: {}", walletId, kid,
        authorizationCode);
    redisTemplate.opsForValue().set(buildKey(walletId, kid), authorizationCode,
        Duration.ofMinutes(10));
  }

  @Override
  public Optional<String> getAuthorizationCode(UUID walletId, String kid) {
    String authCode = redisTemplate.opsForValue().get(buildKey(walletId, kid));
    if (authCode != null) {
      log.info("Get authorization code for walletId: {}, kid: {} code: {}", walletId, kid,
          authCode);
      return Optional.of(authCode);
    } else {
      return Optional.empty();
    }
  }

  private String buildKey(UUID walletId, String kid) {
    return String.format("authCode|%s|%s", walletId.toString(), kid);
  }
}
