package se.digg.wallet.r2ps.domain.mapper;

import com.fasterxml.jackson.core.JacksonException;
import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.databind.DeserializationContext;
import com.fasterxml.jackson.databind.JsonDeserializer;

import java.io.IOException;
import java.time.Instant;

public class InstantMillisDeserializer extends JsonDeserializer<Instant> {
  @Override
  public Instant deserialize(final JsonParser jsonParser,
      final DeserializationContext deserializationContext)
      throws IOException, JacksonException {
    return Instant.ofEpochMilli(jsonParser.getLongValue());
  }
}
