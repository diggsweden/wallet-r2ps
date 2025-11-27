package se.digg.wallet.r2ps.infrastructure.frdemo;

import java.security.KeyStore;
import java.security.PrivateKey;

public record KeyCacheRecord(
    KeyStore keyStore,
    char[] pin,
    String alias,
    PrivateKey privateKey) {
}
