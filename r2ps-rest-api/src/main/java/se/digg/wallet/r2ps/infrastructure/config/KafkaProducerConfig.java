package se.digg.wallet.r2ps.infrastructure.config;

import org.apache.kafka.clients.consumer.ConsumerConfig;
import org.apache.kafka.clients.producer.ProducerConfig;
import org.apache.kafka.common.serialization.StringDeserializer;
import org.apache.kafka.common.serialization.StringSerializer;
import org.springframework.boot.autoconfigure.kafka.KafkaProperties;
import org.springframework.context.annotation.Bean;
import org.springframework.context.annotation.Configuration;
import org.springframework.kafka.core.ConsumerFactory;
import org.springframework.kafka.core.DefaultKafkaConsumerFactory;
import org.springframework.kafka.core.DefaultKafkaProducerFactory;
import org.springframework.kafka.core.KafkaTemplate;
import org.springframework.kafka.core.ProducerFactory;
import org.springframework.kafka.support.serializer.JsonDeserializer;
import org.springframework.kafka.support.serializer.JsonSerializer;
import se.digg.wallet.r2ps.domain.command.Command;
import se.digg.wallet.r2ps.domain.model.R2psRequest;
import java.util.HashMap;
import java.util.Map;

@Configuration
public class KafkaProducerConfig {

  private final KafkaProperties kafkaProperties;

  public KafkaProducerConfig(KafkaProperties kafkaProperties) {
    this.kafkaProperties = kafkaProperties;
  }

  @Bean
  public ProducerFactory<String, String> producerFactoryString() {
    Map<String, Object> configProps = new HashMap<>(kafkaProperties.buildProducerProperties(null));
    configProps.put(
        ProducerConfig.KEY_SERIALIZER_CLASS_CONFIG,
        StringSerializer.class);
    configProps.put(
        ProducerConfig.VALUE_SERIALIZER_CLASS_CONFIG,
        String.class);
    return new DefaultKafkaProducerFactory<>(configProps);
  }

  @Bean
  public ProducerFactory<String, R2psRequest> producerFactoryR2psRequest() {
    Map<String, Object> configProps = new HashMap<>(kafkaProperties.buildProducerProperties(null));
    configProps.put(
        ProducerConfig.KEY_SERIALIZER_CLASS_CONFIG,
        StringSerializer.class);
    configProps.put(
        ProducerConfig.VALUE_SERIALIZER_CLASS_CONFIG,
        JsonSerializer.class);
    return new DefaultKafkaProducerFactory<>(configProps);
  }

  @Bean
  public ProducerFactory<String, Command> producerFactoryCommand() {
    Map<String, Object> configProps = new HashMap<>(kafkaProperties.buildProducerProperties(null));
    configProps.put(
        ProducerConfig.KEY_SERIALIZER_CLASS_CONFIG,
        StringSerializer.class);
    configProps.put(
        ProducerConfig.VALUE_SERIALIZER_CLASS_CONFIG,
        JsonSerializer.class);
    return new DefaultKafkaProducerFactory<>(configProps);
  }

  @Bean
  public ConsumerFactory<String, R2psRequest> consumerFactoryR2psRequest() {
    Map<String, Object> configProps = new HashMap<>(kafkaProperties.buildProducerProperties(null));
    configProps.put(
        ConsumerConfig.KEY_DESERIALIZER_CLASS_CONFIG,
        StringDeserializer.class);
    configProps.put(
        ConsumerConfig.VALUE_DESERIALIZER_CLASS_CONFIG,
        JsonDeserializer.class);
    // JSON Deserializer configuration
    configProps.put(JsonDeserializer.TRUSTED_PACKAGES, "*");
    configProps.put(JsonDeserializer.VALUE_DEFAULT_TYPE, R2psRequest.class.getName());
    configProps.put(JsonDeserializer.USE_TYPE_INFO_HEADERS, false);
    return new DefaultKafkaConsumerFactory<>(configProps, new StringDeserializer(),
        new JsonDeserializer<>(
            R2psRequest.class, false));
  }

  @Bean
  public ConsumerFactory<String, Command> consumerFactoryCommand() {
    Map<String, Object> configProps = new HashMap<>(kafkaProperties.buildProducerProperties(null));
    configProps.put(
        ConsumerConfig.KEY_DESERIALIZER_CLASS_CONFIG,
        StringDeserializer.class);
    configProps.put(
        ConsumerConfig.VALUE_DESERIALIZER_CLASS_CONFIG,
        JsonDeserializer.class);
    // JSON Deserializer configuration
    configProps.put(JsonDeserializer.TRUSTED_PACKAGES, "*");
    configProps.put(JsonDeserializer.VALUE_DEFAULT_TYPE, R2psRequest.class.getName());
    configProps.put(JsonDeserializer.USE_TYPE_INFO_HEADERS, false);
    return new DefaultKafkaConsumerFactory<>(configProps, new StringDeserializer(),
        new JsonDeserializer<>(
            Command.class, false));
  }

  @Bean
  public ConsumerFactory<String, String> consumerFactoryString() {
    Map<String, Object> configProps = new HashMap<>(kafkaProperties.buildProducerProperties(null));
    configProps.put(
        ConsumerConfig.KEY_DESERIALIZER_CLASS_CONFIG,
        StringDeserializer.class);
    configProps.put(
        ConsumerConfig.VALUE_DESERIALIZER_CLASS_CONFIG,
        StringDeserializer.class);
    // JSON Deserializer configuration
    configProps.put(JsonDeserializer.TRUSTED_PACKAGES, "*");
    configProps.put(JsonDeserializer.VALUE_DEFAULT_TYPE, String.class.getName());
    configProps.put(JsonDeserializer.USE_TYPE_INFO_HEADERS, false);
    return new DefaultKafkaConsumerFactory<>(configProps, new StringDeserializer(),
        new StringDeserializer());
  }


  @Bean
  public KafkaTemplate<String, R2psRequest> kafkaTemplateR2psRequest(
      ProducerFactory<String, R2psRequest> producerFactory) {
    return new KafkaTemplate<>(producerFactory);
  }

  @Bean
  public KafkaTemplate<String, Command> kafkaTemplateCommand(
      ProducerFactory<String, Command> producerFactory) {
    return new KafkaTemplate<>(producerFactory);
  }


  @Bean
  public KafkaTemplate<String, String> kafkaTemplateString(
      ProducerFactory<String, String> producerFactory) {
    return new KafkaTemplate<>(producerFactory);
  }
}
