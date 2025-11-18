package se.digg.wallet.r2ps.domain.mapper;

import com.fasterxml.jackson.core.JsonGenerator;
import com.fasterxml.jackson.databind.JsonSerializer;
import com.fasterxml.jackson.databind.SerializerProvider;

import java.io.IOException;
import java.security.PublicKey;
import java.util.Base64;

public class PublicKeySerializer extends JsonSerializer<PublicKey> {
  @Override
  public void serialize(PublicKey publicKey, JsonGenerator jsonGenerator,
      SerializerProvider serializerProvider)
      throws IOException {
    jsonGenerator.writeString(Base64.getEncoder().encodeToString(publicKey.getEncoded()));
  }
}
