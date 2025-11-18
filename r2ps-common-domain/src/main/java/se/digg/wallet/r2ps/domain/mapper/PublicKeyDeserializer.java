package se.digg.wallet.r2ps.domain.mapper;

import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.databind.DeserializationContext;
import com.fasterxml.jackson.databind.JsonDeserializer;

import java.io.IOException;
import java.security.KeyFactory;
import java.security.PublicKey;
import java.security.spec.InvalidKeySpecException;
import java.security.spec.X509EncodedKeySpec;
import java.util.Base64;

public class PublicKeyDeserializer extends JsonDeserializer<PublicKey> {
  @Override
  public PublicKey deserialize(JsonParser jsonParser, DeserializationContext deserializationContext)
      throws IOException {
    try {
      byte[] keyBytes = Base64.getDecoder().decode(jsonParser.getValueAsString());
      X509EncodedKeySpec keySpec = new X509EncodedKeySpec(keyBytes);

      // Try RSA first
      try {
        return KeyFactory.getInstance("RSA").generatePublic(keySpec);
      } catch (InvalidKeySpecException e) {
        // If RSA fails, try EC
        return KeyFactory.getInstance("EC").generatePublic(keySpec);
      }
    } catch (Exception e) {
      throw new IOException("Failed to deserialize public key", e);
    }
  }
}
