package se.digg.wallet.r2ps.infrastructure.frdemo;

import java.security.cert.X509Certificate;
import java.time.Instant;

public record EcKeyPairRecord(
    /* The key identifier for the key record */
    String kid,
    /* The serialized or wrapped private key bytes */
    byte[] privateKey,
    /* The key store certificate */
    X509Certificate certificate,
    /* Name of the elliptic curve used to identify the curve in the Service API */
    String curveName,
    /* Time of original key creation */
    Instant creationTime) {
}
