package se.digg.wallet.r2ps.infrastructure.config;

import org.springframework.boot.context.properties.ConfigurationProperties;

@ConfigurationProperties("r2ps")
public record R2psKafkaConfig(R2psResponseInConfig in, R2psRequestOutConfig out,
    R2psAsyncRestConfig rest) {
  public R2psKafkaConfig(R2psResponseInConfig in, R2psRequestOutConfig out,
      R2psAsyncRestConfig rest) {
    this.in = in;
    this.out = out;
    this.rest = rest;
  }
}
