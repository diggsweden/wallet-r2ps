package se.digg.wallet.r2ps.infrastructure.config;

import com.fasterxml.jackson.databind.ObjectMapper;
import org.springframework.context.annotation.Bean;
import org.springframework.context.annotation.Configuration;
import org.springframework.data.redis.connection.RedisConnectionFactory;
import org.springframework.data.redis.core.RedisTemplate;
import org.springframework.data.redis.serializer.Jackson2JsonRedisSerializer;
import se.digg.wallet.r2ps.infrastructure.adapter.dto.R2psResponseDto;

@Configuration
class ValKeyConfig {


  private final ObjectMapper mapper;

  public ValKeyConfig(ObjectMapper mapper) {
    this.mapper = mapper;
  }

  @Bean
  RedisTemplate<String, R2psResponseDto> redisTemplate(RedisConnectionFactory connectionFactory) {
    RedisTemplate<String, R2psResponseDto> template = new RedisTemplate<>();
    template.setConnectionFactory(connectionFactory);
    template.setValueSerializer(new Jackson2JsonRedisSerializer<>(mapper, R2psResponseDto.class));
    return template;
  }
}
