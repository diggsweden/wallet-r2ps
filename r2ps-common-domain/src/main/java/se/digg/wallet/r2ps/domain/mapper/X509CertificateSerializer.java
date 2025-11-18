package se.digg.wallet.r2ps.domain.mapper;

import com.fasterxml.jackson.core.JsonGenerator;
import com.fasterxml.jackson.databind.JsonSerializer;
import com.fasterxml.jackson.databind.SerializerProvider;

import java.io.IOException;
import java.security.cert.X509Certificate;

public class X509CertificateSerializer extends JsonSerializer<X509Certificate> {

  @Override
  public void serialize(X509Certificate certificate, JsonGenerator gen, SerializerProvider provider)
      throws IOException {
    try {
      gen.writeStartArray();
      gen.writeString(java.util.Base64.getEncoder().encodeToString(certificate.getEncoded()));
      gen.writeEndArray();
    } catch (Exception e) {
      throw new IOException("Failed to serialize X509 Certificate", e);
    }
  }
}
