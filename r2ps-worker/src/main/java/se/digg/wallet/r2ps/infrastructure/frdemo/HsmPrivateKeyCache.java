package se.digg.wallet.r2ps.infrastructure.frdemo;

import com.github.benmanes.caffeine.cache.*;
import com.github.benmanes.caffeine.cache.stats.CacheStats;
import org.slf4j.Logger;
import se.digg.wallet.r2ps.commons.exception.ServiceRequestException;
import se.digg.wallet.r2ps.infrastructure.config.KeyProviderBundle;

import java.security.PrivateKey;
import java.time.Duration;
import java.util.Objects;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.TimeUnit;

public class HsmPrivateKeyCache implements AutoCloseable {
  private static final Logger log = org.slf4j.LoggerFactory.getLogger(HsmPrivateKeyCache.class);
  private final Cache<String, KeyCacheRecord> cache;
  private final ExecutorService destroyPool;
  private final PrivateKeyWrapper privateKeyWrapper;

  public HsmPrivateKeyCache(Duration idleTtl, long maximumSize,
      PrivateKeyWrapper privateKeyWrapper) {
    this.destroyPool = Executors.newFixedThreadPool(2, r -> {
      Thread t = new Thread(r, "hsm-destroy");
      t.setDaemon(true);
      return t;
    });
    this.privateKeyWrapper = privateKeyWrapper;
    this.cache = Caffeine.newBuilder()
        .expireAfterAccess(idleTtl) // refreshes on every get
        .maximumSize(maximumSize)
        .recordStats()
        .scheduler(Scheduler.systemScheduler())
        .removalListener((String kid, KeyCacheRecord cacheRecord, RemovalCause cause) -> {
          if (cacheRecord == null) {
            return;
          }
          log.debug("Removal listener for cache triggered for cause: {}", cause.name());
          // Only delete on expiration/size pressure; keep delete on explicit invalidate too? your
          // call:
          boolean shouldDelete =
              cause == RemovalCause.EXPIRED ||
                  cause == RemovalCause.SIZE ||
                  cause == RemovalCause.EXPLICIT;
          if (!shouldDelete) {
            log.debug("Not deleting private key from HSM - {}", cause.name());
            return;
          }
          destroyPool.execute(() -> {
            try {
              log.debug("Removing private key from HSM and from cache");
              privateKeyWrapper.deleteKeyFromHsm(cacheRecord);
            } catch (ServiceRequestException e) {
              log.warn("Failed to delete key '{}' from HSM on eviction (cause={}): {}", kid, cause,
                  e.getMessage(), e);
            } catch (Throwable t) {
              log.warn("Unexpected error deleting key '{}' from HSM (cause={})", kid, cause, t);
            }
          });
        })
        .build();
    Runtime.getRuntime().addShutdownHook(new Thread(() -> {
      log.info("Closing HSM private key cache");
      cache.invalidateAll(); // triggers removalListener
      cache.cleanUp();
      destroyPool.shutdown();
    }));
  }

  @Override
  public void close() {
    cache.invalidateAll();
    cache.cleanUp();
    destroyPool.shutdown();
    try {
      if (!destroyPool.awaitTermination(5, TimeUnit.SECONDS)) {
        destroyPool.shutdownNow();
      }
    } catch (InterruptedException e) {
      // Restore the interrupt flag if interrupted
      Thread.currentThread().interrupt();
      destroyPool.shutdownNow();
    }
  }

  public boolean isKeyInCache(String kid) {
    Objects.requireNonNull(kid);
    return cache.policy().getEntryIfPresentQuietly(kid) != null;
  }

  public PrivateKey getKey(String kid) {
    Objects.requireNonNull(kid);
    KeyCacheRecord record = cache.getIfPresent(kid);
    if (record == null) {
      return null;
    }
    return record.privateKey();
  }

  public void putKey(String kid, PrivateKey key, KeyProviderBundle kpBundle) {
    Objects.requireNonNull(kid);
    Objects.requireNonNull(key);
    Objects.requireNonNull(kpBundle);
    KeyCacheRecord record = new KeyCacheRecord(
        kpBundle.getKeyStore(),
        kpBundle.getKsPassword(),
        kid,
        key);
    cache.asMap().putIfAbsent(kid, record);
  }

  public void invalidate(String kid) {
    Objects.requireNonNull(kid);
    cache.invalidate(kid);
  }

  public CacheStats stats() {
    return cache.stats();
  }
}
