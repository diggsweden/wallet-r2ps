package se.digg.wallet.r2ps.infrastructure.config;

import com.fasterxml.jackson.core.type.TypeReference;
import com.fasterxml.jackson.databind.JavaType;
import com.fasterxml.jackson.databind.ObjectMapper;
import org.springframework.context.annotation.Bean;
import org.springframework.context.annotation.Configuration;
import org.springframework.data.redis.connection.RedisConnectionFactory;
import org.springframework.data.redis.core.RedisTemplate;
import org.springframework.data.redis.serializer.Jackson2JsonRedisSerializer;
import se.digg.wallet.r2ps.domain.aggregate.ServerWallet;
import se.digg.wallet.r2ps.domain.event.Event;
import se.digg.wallet.r2ps.domain.model.R2psResponse;

import java.util.List;
import java.util.UUID;

@Configuration
class ValKeyConfig {


  private final ObjectMapper mapper;

  public ValKeyConfig(ObjectMapper mapper) {
    this.mapper = mapper;
  }

  @Bean
  RedisTemplate<String, R2psResponse> redisTemplate(RedisConnectionFactory connectionFactory) {
    RedisTemplate<String, R2psResponse> template = new RedisTemplate<>();
    template.setConnectionFactory(connectionFactory);
    template.setValueSerializer(new Jackson2JsonRedisSerializer<>(mapper, R2psResponse.class));
    return template;
  }

  @Bean
  RedisTemplate<UUID, List<Event>> redisTemplateEvent(RedisConnectionFactory connectionFactory) {
    RedisTemplate<UUID, List<Event>> template = new RedisTemplate<>();
    template.setConnectionFactory(connectionFactory);
    JavaType javaType = mapper.getTypeFactory().constructCollectionType(List.class, Event.class);
    template.setValueSerializer(new Jackson2JsonRedisSerializer<>(mapper, javaType));
    return template;
  }

  @Bean
  RedisTemplate<UUID, ServerWallet> redisTemplateServerWallet(
      RedisConnectionFactory connectionFactory) {
    RedisTemplate<UUID, ServerWallet> template = new RedisTemplate<>();
    template.setConnectionFactory(connectionFactory);
    template.setValueSerializer(new Jackson2JsonRedisSerializer<>(mapper, ServerWallet.class));
    return template;
  }

  @Bean
  RedisTemplate<String, ServerWallet> redisTemplateStringServerWallet(
      RedisConnectionFactory connectionFactory) {
    RedisTemplate<String, ServerWallet> template = new RedisTemplate<>();
    template.setConnectionFactory(connectionFactory);
    template.setValueSerializer(new Jackson2JsonRedisSerializer<>(mapper, ServerWallet.class));
    return template;
  }

}
