package se.digg.wallet.r2ps.domain.mapper;

import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.databind.DeserializationContext;
import com.fasterxml.jackson.databind.JsonDeserializer;
import com.fasterxml.jackson.databind.JsonNode;
import org.bouncycastle.util.encoders.Base64;

import java.io.ByteArrayInputStream;
import java.io.IOException;
import java.security.NoSuchProviderException;
import java.security.cert.CertificateException;
import java.security.cert.CertificateFactory;
import java.security.cert.X509Certificate;

public class X509CertificateDeserializer extends JsonDeserializer<X509Certificate> {

  @Override
  public X509Certificate deserialize(JsonParser jsonParser, DeserializationContext ctxt)
      throws IOException {
    try {
      JsonNode node = jsonParser.getCodec().readTree(jsonParser);
      if (!node.isArray() || node.size() < 1) {
        throw new IOException("Input must be an array with at least one certificate");
      }
      String certBase64 = node.get(0).asText();
      byte[] certBytes = Base64.decode(certBase64);
      CertificateFactory cf = CertificateFactory.getInstance("X.509", "BC");
      return (X509Certificate) cf.generateCertificate(new ByteArrayInputStream(certBytes));
    } catch (CertificateException | NoSuchProviderException e) {
      throw new IOException("Failed to deserialize X.509Certificate", e);
    }
  }
}
