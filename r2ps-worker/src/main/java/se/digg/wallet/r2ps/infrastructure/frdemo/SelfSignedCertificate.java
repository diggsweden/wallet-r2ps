package se.digg.wallet.r2ps.infrastructure.frdemo;

import org.bouncycastle.asn1.x500.X500Name;
import org.bouncycastle.cert.X509v3CertificateBuilder;
import org.bouncycastle.cert.jcajce.JcaX509CertificateConverter;
import org.bouncycastle.cert.jcajce.JcaX509v3CertificateBuilder;
import org.bouncycastle.operator.ContentSigner;
import org.bouncycastle.operator.jcajce.JcaContentSignerBuilder;

import java.math.BigInteger;
import java.security.KeyPair;
import java.security.Provider;
import java.security.SecureRandom;
import java.security.cert.X509Certificate;
import java.time.Duration;
import java.time.Instant;
import java.util.Date;

public class SelfSignedCertificate {

  public static SecureRandom RNG = new SecureRandom();

  public static X509Certificate create(KeyPair kp, String subjectDn, int days, Provider provider)
      throws Exception {
    long now = System.currentTimeMillis();
    Date start = new Date(now);
    Date end = Date.from(Instant.now().plus(Duration.ofDays(days)));

    X500Name dnName = new X500Name(subjectDn);
    BigInteger certSerial = new BigInteger(64, RNG);

    X509v3CertificateBuilder certBuilder = new JcaX509v3CertificateBuilder(
        dnName, certSerial, start, end, dnName, kp.getPublic());

    ContentSigner signer = new JcaContentSignerBuilder("SHA256withECDSA")
        .setProvider(provider)
        .build(kp.getPrivate());

    return new JcaX509CertificateConverter()
        .setProvider("BC")
        .getCertificate(certBuilder.build(signer));
  }
}
