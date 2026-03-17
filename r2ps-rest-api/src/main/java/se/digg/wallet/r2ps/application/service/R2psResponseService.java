package se.digg.wallet.r2ps.application.service;

import java.util.Optional;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import se.digg.wallet.r2ps.application.port.in.R2psResponseUseCase;
import se.digg.wallet.r2ps.application.port.out.R2psResponseSinkSpiPort;
import se.digg.wallet.r2ps.domain.model.R2psResponse;

/**
 * Bridges the Kafka consumer thread and HTTP request threads using
 * in-memory {@link CompletableFuture} signaling. When a response arrives
 * from Kafka, any HTTP thread blocking on the same correlationId is
 * woken up immediately — no polling delay.
 *
 * <p>Responses are also stored in Valkey so that {@code GET /task/{id}}
 * polling clients continue to work.
 */
public class R2psResponseService implements R2psResponseUseCase {

  private static final Logger log = LoggerFactory.getLogger(R2psResponseService.class);

  /** Interval between Valkey polls when the CompletableFuture doesn't complete (multi-pod fallback). */
  private static final long POLL_INTERVAL_MS = 100;

  private final R2psResponseSinkSpiPort r2psResponseSinkSpiPort;

  /** Waiting HTTP threads keyed by correlationId. */
  private final ConcurrentHashMap<String, CompletableFuture<R2psResponse>> pendingRequests =
      new ConcurrentHashMap<>();

  public R2psResponseService(R2psResponseSinkSpiPort r2psResponseSinkSpiPort) {
    this.r2psResponseSinkSpiPort = r2psResponseSinkSpiPort;
  }

  /**
   * Called by the Kafka consumer thread when a worker response arrives.
   * Stores the response in Valkey and completes any waiting future.
   */
  @Override
  public void r2psResponseReady(R2psResponse r2psResponse) {
    // Store in Valkey (for GET /task/{id} polling clients)
    r2psResponseSinkSpiPort.storeResponse(r2psResponse);

    // Wake up the waiting HTTP thread, if any
    CompletableFuture<R2psResponse> future = pendingRequests.remove(r2psResponse.correlationId());
    if (future != null) {
      future.complete(r2psResponse);
    }
  }

  /**
   * Called by HTTP threads to wait for a response. Registers a
   * {@link CompletableFuture} and blocks until the Kafka consumer
   * completes it or the timeout expires.
   *
   * <p>A single Valkey check is performed first to handle the race
   * where the response arrived before the future was registered.
   */
  /**
   * Hybrid wait: uses {@link CompletableFuture} for same-pod fast path (~1-2ms)
   * and falls back to periodic Valkey polling for multi-pod deployments where
   * a different pod's Kafka consumer may have received the response.
   */
  @Override
  public Optional<R2psResponse> waitForR2psResponse(String correlationId, long timeoutMillis) {
    CompletableFuture<R2psResponse> future = new CompletableFuture<>();
    pendingRequests.put(correlationId, future);

    long deadline = System.currentTimeMillis() + timeoutMillis;

    try {
      // Race-condition guard: check if already in Valkey
      Optional<R2psResponse> existing = r2psResponseSinkSpiPort.loadResponse(correlationId);
      if (existing.isPresent()) {
        log.info("Got r2psResponse for correlationId={} (already in Valkey)", correlationId);
        return existing;
      }

      // Hybrid wait loop: try CompletableFuture with short timeout, poll Valkey as fallback
      while (System.currentTimeMillis() < deadline) {
        try {
          // Same-pod fast path: Kafka consumer on this pod completes the future
          R2psResponse response = future.get(POLL_INTERVAL_MS, TimeUnit.MILLISECONDS);
          log.info("Got r2psResponse for correlationId={} (same-pod fast path)", correlationId);
          return Optional.of(response);
        } catch (TimeoutException e) {
          // Future didn't complete → check Valkey (multi-pod fallback)
          Optional<R2psResponse> fromValkey = r2psResponseSinkSpiPort.loadResponse(correlationId);
          if (fromValkey.isPresent()) {
            log.info("Got r2psResponse for correlationId={} (multi-pod Valkey fallback)", correlationId);
            return fromValkey;
          }
          // Response not available yet, continue waiting
        }
      }

      // Overall timeout expired
      log.info("Timeout waiting for response for correlationId: {}", correlationId);
      return Optional.empty();
    } catch (InterruptedException e) {
      Thread.currentThread().interrupt();
      log.warn("Interrupted while waiting for response for correlationId={}", correlationId);
      return Optional.empty();
    } catch (ExecutionException e) {
      log.warn("Error waiting for response for correlationId={}: {}", correlationId, e.getMessage());
      return Optional.empty();
    } finally {
      pendingRequests.remove(correlationId);
    }
  }
}
