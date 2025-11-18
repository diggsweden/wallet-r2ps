package se.digg.wallet.r2ps.infrastructure.config;

import org.springframework.boot.context.properties.EnableConfigurationProperties;
import org.springframework.context.annotation.Configuration;

@Configuration
@EnableConfigurationProperties(R2psKafkaConfig.class)
public class Config {

  private final R2psKafkaConfig kafkaConfig;

  public Config(R2psKafkaConfig kafkaConfig) {
    this.kafkaConfig = kafkaConfig;
  }

  public R2psKafkaConfig getKafka() {
    return kafkaConfig;
  }
}
