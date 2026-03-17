package se.digg.wallet.r2ps.infrastructure.config;

import org.springframework.boot.context.properties.EnableConfigurationProperties;
import org.springframework.context.annotation.Bean;
import org.springframework.context.annotation.Configuration;
import se.digg.wallet.r2ps.application.port.in.R2psResponseUseCase;
import se.digg.wallet.r2ps.application.port.out.R2psResponseSinkSpiPort;
import se.digg.wallet.r2ps.application.service.R2psResponseService;

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

  @Bean
  public R2psResponseUseCase r2psResponseUseCase(R2psResponseSinkSpiPort r2psResponseSinkSpiPort) {
    return new R2psResponseService(r2psResponseSinkSpiPort);
  }
}
