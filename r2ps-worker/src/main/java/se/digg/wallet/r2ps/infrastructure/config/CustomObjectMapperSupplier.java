package se.digg.wallet.r2ps.infrastructure.config;

import com.fasterxml.jackson.databind.ObjectMapper;
import io.hypersistence.utils.hibernate.type.util.ObjectMapperSupplier;
import org.springframework.stereotype.Component;

import java.util.TimeZone;

@Component
public class CustomObjectMapperSupplier
    implements ObjectMapperSupplier {

  private final ObjectMapper objectMapper;

  public CustomObjectMapperSupplier(ObjectMapper objectMapper) {
    this.objectMapper = objectMapper;
  }

  @Override
  public ObjectMapper get() {
    return objectMapper;
  }
}
