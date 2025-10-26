package se.digg.wallet.r2ps.infrastructure.config;

import org.springframework.boot.context.properties.ConfigurationProperties;

@ConfigurationProperties("r2ps.worker")
public record R2psWorkerConfig(R2psWorkerInConfig in, R2psWorkerOutConfig out) {

  public R2psWorkerConfig(R2psWorkerInConfig in, R2psWorkerOutConfig out) {
    this.in = in;
    this.out = out;
  }
}
