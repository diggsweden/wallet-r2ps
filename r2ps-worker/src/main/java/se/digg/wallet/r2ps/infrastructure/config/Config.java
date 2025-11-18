package se.digg.wallet.r2ps.infrastructure.config;

import org.springframework.boot.context.properties.EnableConfigurationProperties;
import org.springframework.context.annotation.Configuration;

@Configuration
@EnableConfigurationProperties(R2psWorkerConfig.class)
public class Config {

  private final R2psWorkerConfig workerConfig;

  public Config(R2psWorkerConfig workerConfig) {
    this.workerConfig = workerConfig;
  }

  public R2psWorkerConfig getWorker() {
    return workerConfig;
  }
}
