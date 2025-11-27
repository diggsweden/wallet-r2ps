package se.digg.wallet.r2ps.infrastructure.frdemo.impl;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.core.type.TypeReference;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.json.JsonMapper;
import com.fasterxml.jackson.databind.module.SimpleModule;
import org.slf4j.Logger;
import se.digg.wallet.r2ps.commons.exception.ServiceRequestException;
import se.digg.wallet.r2ps.commons.serializers.InstantMillisDeserializer;
import se.digg.wallet.r2ps.commons.serializers.InstantMillisSerializer;
import se.digg.wallet.r2ps.commons.serializers.X509CertificateDeserializer;
import se.digg.wallet.r2ps.commons.serializers.X509CertificateSerializer;
import se.digg.wallet.r2ps.infrastructure.config.KeyProviderBundle;
import se.digg.wallet.r2ps.infrastructure.frdemo.EcKeyPairRecord;
import se.digg.wallet.r2ps.infrastructure.frdemo.EcKeyPairRecordRegistry;
import se.digg.wallet.r2ps.infrastructure.frdemo.HsmPrivateKeyCache;
import se.digg.wallet.r2ps.infrastructure.frdemo.KeyCacheRecord;
import se.digg.wallet.r2ps.infrastructure.frdemo.KeyStoreStrategy;
import se.digg.wallet.r2ps.infrastructure.frdemo.PrivateKeyWrapper;
import se.digg.wallet.r2ps.infrastructure.frdemo.SelfSignedCertificate;

import java.io.File;
import java.io.IOException;
import java.io.OutputStream;
import java.math.BigInteger;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.security.KeyPair;
import java.security.KeyPairGenerator;
import java.security.KeyStore;
import java.security.KeyStoreException;
import java.security.NoSuchAlgorithmException;
import java.security.PrivateKey;
import java.security.SecureRandom;
import java.security.UnrecoverableKeyException;
import java.security.cert.Certificate;
import java.security.cert.CertificateException;
import java.security.cert.X509Certificate;
import java.time.Instant;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Optional;
import java.util.Random;

public class GenericEcKeyPairRecordRegistry implements EcKeyPairRecordRegistry, AutoCloseable {

  private final static String REGISTRY_FILE_NAME = "wallet-key-registry.json";
  private final static Random RNG = new SecureRandom();
  private final static ObjectMapper OBJECT_MAPPER;
  private static final Logger log =
      org.slf4j.LoggerFactory.getLogger(GenericEcKeyPairRecordRegistry.class);

  static {
    OBJECT_MAPPER = JsonMapper.builder()
        .serializationInclusion(JsonInclude.Include.NON_NULL)
        .addModule(new SimpleModule()
            .addSerializer(X509Certificate.class, new X509CertificateSerializer())
            .addDeserializer(X509Certificate.class, new X509CertificateDeserializer())
            .addSerializer(Instant.class, new InstantMillisSerializer())
            .addDeserializer(Instant.class, new InstantMillisDeserializer()))
        .build();
  }

  private final File registryFile;
  private final Map<String, Map<String, EcKeyPairRecord>> records;
  private final HsmPrivateKeyCache privateKeyCache;
  private final Map<String, KeyProviderBundle> keyProviderBundles;
  private final PrivateKeyWrapper keyWrapper;
  private final String keyWrapAlias;

  public GenericEcKeyPairRecordRegistry(final Map<String, KeyProviderBundle> keyProviderBundles,
      final File configFile, final HsmPrivateKeyCache privateKeyCache, String keyWrapAlias,
      final PrivateKeyWrapper keyWrapper) {
    this.keyProviderBundles = keyProviderBundles;
    this.registryFile = new File(configFile, REGISTRY_FILE_NAME);
    this.privateKeyCache = privateKeyCache;
    if (!registryFile.exists()) {
      configFile.mkdirs();
    }
    this.keyWrapAlias = keyWrapAlias;
    this.keyWrapper = keyWrapper;
    this.records = new HashMap<>();
    loadRegistry();
  }

  private void loadRegistry() {
    if (registryFile.exists()) {
      try {
        final Map<String, Map<String, EcKeyPairRecord>> loadedRecords =
            OBJECT_MAPPER.readValue(registryFile,
                new TypeReference<Map<String, Map<String, EcKeyPairRecord>>>() {});
        this.records.putAll(loadedRecords);
        // Remove dangling keys from keyStores
        synchronize();
      } catch (Exception e) {
        throw new RuntimeException(e);
      }
    }
  }

  private synchronized void saveRegistry() {
    try {
      OBJECT_MAPPER.writeValue(registryFile, records);
    } catch (Exception e) {
      throw new RuntimeException(e);
    }
  }

  private void backupKeyStore(KeyProviderBundle kpBundle)
      throws IOException, CertificateException, KeyStoreException, NoSuchAlgorithmException {
    if (kpBundle.getKsLocation() != null
        && kpBundle.getKeyStoreStrategy() == KeyStoreStrategy.objects) {
      // Save back to the same file (atomic write shown below)
      Path ksPath = kpBundle.getKsLocation().toPath();
      Path tmp = ksPath.resolveSibling(ksPath.getFileName() + ".tmp");
      try (OutputStream out = Files.newOutputStream(tmp)) {
        kpBundle.getKeyStore().store(out, kpBundle.getKsPassword());
      }
      // Replace original atomically
      Files.move(tmp, ksPath, StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
      log.debug("Stored updated wallet key key store");
    }
  }

  @Override
  public void close() throws Exception {
    privateKeyCache.close();
  }

  @Override
  public Optional<EcKeyPairRecord> getRecord(final String clientId, final String kid)
      throws ServiceRequestException {
    validateNonNull(clientId, kid);
    if (!records.containsKey(clientId)) {
      log.debug("Found no record for client {}", clientId);
      return Optional.empty();
    }
    if (!records.get(clientId).containsKey(kid)) {
      log.debug("Found no record for ClientId: {} with kid: {}", clientId, kid);
      return Optional.empty();
    }
    return Optional.of(records.get(clientId).get(kid));
  }

  @Override
  public List<EcKeyPairRecord> getClientRecords(final String clientId) {
    if (!records.containsKey(clientId)) {
      return new ArrayList<>();
    }
    return records.get(clientId).values().stream().toList();
  }

  @Override
  public KeyPair getKey(final String clientId, final String kid) throws ServiceRequestException {
    validateNonNull(clientId, kid);
    try {
      final Optional<EcKeyPairRecord> recordOptional = getRecord(clientId, kid);
      // This key is a permanent key object in the key store
      if (recordOptional.isEmpty()) {
        log.debug("Found no record for client {}", clientId);
        throw new ServiceRequestException("No such key");
      }
      EcKeyPairRecord record = recordOptional.get();
      final KeyProviderBundle kpBundle = keyProviderBundles.get(recordOptional.get().curveName());
      if (kpBundle == null) {
        throw new ServiceRequestException("No such key provider");
      }
      if (kpBundle.getKeyStoreStrategy().equals(KeyStoreStrategy.objects)) {
        final PrivateKey key =
            (PrivateKey) kpBundle.getKeyStore().getKey(kid, kpBundle.getKsPassword());
        return new KeyPair(record.certificate().getPublicKey(), key);
      }
      // This key is not in the KeyStore. It is only wrapped by the HSM and stored outside as an
      // encrypted key.
      // Attempt to find the PrivateKey handle in the cache?
      PrivateKey cachedPrivateKey = privateKeyCache.getKey(kid);
      if (cachedPrivateKey != null) {
        return new KeyPair(record.certificate().getPublicKey(), cachedPrivateKey);
      }
      // It is not in the cache. Unwrap it and put the PrivateKey handle it in the cache under its
      // kid.
      PrivateKey unwrappedPrivateKey = keyWrapper.unwrapKey(record, kid, kpBundle);
      privateKeyCache.putKey(kid, unwrappedPrivateKey, kpBundle);
      // This key will be usable until its TTL is reached, then deleted from the HSM and the cache.
      // The TTL is reset after each getKey(), making the TTL a max idle time before it's removed.
      return new KeyPair(record.certificate().getPublicKey(), unwrappedPrivateKey);
    } catch (UnrecoverableKeyException | KeyStoreException | NoSuchAlgorithmException
        | ClassCastException e) {
      throw new ServiceRequestException("Unable to retrieve key: " + e.getMessage(), e);
    }
  }

  @Override
  public void generateAndStoreKey(final String clientId, final KeyProviderBundle kpBundle)
      throws ServiceRequestException {
    validateNonNull(clientId, kpBundle);
    String kid = new BigInteger(128, RNG).toString(32);
    String curve = kpBundle.getCurve();
    final KeyStore keyStore = kpBundle.getKeyStore();
    try {
      final Optional<EcKeyPairRecord> recordOptional = getRecord(clientId, kid);
      if (recordOptional.isPresent()) {
        throw new ServiceRequestException("Key already exists");
      }
      EcKeyPairRecord keyPairRecord;
      if (kpBundle.getKeyStoreStrategy().equals(KeyStoreStrategy.objects)) {
        final KeyPairGenerator keyPairGenerator = kpBundle.getKeyPairGenerator();
        final KeyPair keyPair = keyPairGenerator.generateKeyPair();
        X509Certificate cert =
            SelfSignedCertificate.create(keyPair, "CN=" + kid, 30, kpBundle.getProvider());
        keyStore.setKeyEntry(kid, keyPair.getPrivate(), kpBundle.getKsPassword(),
            new Certificate[] {cert});
        keyPairRecord = new EcKeyPairRecord(kid, null, cert, curve, Instant.now());
      } else {
        keyWrapper.generateKey(kid, kpBundle);
        X509Certificate cert = (X509Certificate) keyStore.getCertificate(kid);
        byte[] wrappedKeyBytes = keyWrapper.wrapKey(kid, kpBundle);
        keyPairRecord = new EcKeyPairRecord(kid, wrappedKeyBytes, cert, curve, Instant.now());
        keyWrapper.deleteKeyFromHsm(
            new KeyCacheRecord(keyStore, kpBundle.getKsPassword(), kid, null));
      }
      // Update the registry
      Map<String, EcKeyPairRecord> clientKeys =
          records.computeIfAbsent(clientId, k -> new HashMap<>());
      clientKeys.put(kid, keyPairRecord);
      saveRegistry();
      backupKeyStore(kpBundle);
    } catch (Exception e) {
      throw new ServiceRequestException("Failed to store key: " + e.getMessage(), e);
    }
  }

  @Override
  public void deleteKey(final String clientId, final String kid) throws ServiceRequestException {
    validateNonNull(clientId, kid);
    try {
      Map<String, EcKeyPairRecord> clientRecords = records.get(clientId);
      if (clientRecords == null) {
        log.debug("Found no record for client {}", clientId);
        throw new ServiceRequestException("No such client");
      }
      final EcKeyPairRecord record = clientRecords.get(kid);
      if (record == null) {
        log.debug("Found no record for client {} to delete with kid {}", clientId, kid);
        throw new ServiceRequestException("No such key");
      }
      final KeyProviderBundle kpBundle = keyProviderBundles.get(record.curveName());
      if (kpBundle == null) {
        throw new ServiceRequestException("No such key provider");
      }
      if (kpBundle.getKeyStoreStrategy().equals(KeyStoreStrategy.objects)) {
        kpBundle.getKeyStore().deleteEntry(kid);
      } else {
        privateKeyCache.invalidate(kid);
      }
      clientRecords.remove(kid);
      saveRegistry();
      backupKeyStore(kpBundle);
    } catch (KeyStoreException | IOException | CertificateException | NoSuchAlgorithmException e) {
      throw new RuntimeException(e);
    }
  }

  @Override
  public void synchronize() throws ServiceRequestException {
    try {
      for (KeyProviderBundle kpBundle : keyProviderBundles.values()) {
        if (kpBundle.getKeyStoreStrategy() == KeyStoreStrategy.wrapped) {
          continue;
        }
        KeyStore keyStore = kpBundle.getKeyStore();
        for (String kid : Collections.list(keyStore.aliases())) {
          if (Objects.equals(kid, keyWrapAlias)) {
            continue;
          }
          boolean inUse = false;
          outer: for (Map<String, EcKeyPairRecord> map : records.values()) {
            for (EcKeyPairRecord rec : map.values()) {
              if (kid.equals(rec.kid())) {
                inUse = true;
                break outer;
              }
            }
          }
          if (!inUse) {
            kpBundle.getKeyStore().deleteEntry(kid);
          }
        }
      }
    } catch (KeyStoreException e) {
      throw new RuntimeException(e);
    }
  }

  @Override
  public long numberOfKeys(final String clientId, final String curve) {
    final Map<String, EcKeyPairRecord> clientKeyPairs = records.get(clientId);
    if (clientKeyPairs == null) {
      return 0;
    }
    return clientKeyPairs.values().stream()
        .filter(keyPair -> keyPair.curveName().equals(curve))
        .count();

  }

  /*
   * private byte[] wrapPrivateKey(final KeyProviderBundle kpBundle, final PrivateKey privateKey)
   * throws NoSuchPaddingException, NoSuchAlgorithmException, InvalidKeyException,
   * IllegalBlockSizeException, IOException, InvalidKeySpecException,
   * InvalidAlgorithmParameterException { Provider p11Provider = kpBundle.getProvider(); PublicKey
   * kekPub = getKekPublicKey();
   *
   * // List ciphers: log.debug("Available ciphers:\n{}", p11Provider.getServices().stream()
   * .filter(s -> s.getType().equals("Cipher")) .map(s -> s.getAlgorithm()) .sorted().toList());
   *
   * var oaep = new OAEPParameterSpec("SHA-1", "MGF1", MGF1ParameterSpec.SHA1,
   * PSource.PSpecified.DEFAULT); Cipher wrapC =
   * Cipher.getInstance("RSA/ECB/OAEPWithSHA-1AndMGF1Padding", p11Provider);
   * wrapC.init(Cipher.WRAP_MODE, kekPub, oaep); return wrapC.wrap(privateKey); // returns PKCS#8
   * wrapped by RSA-OAEP }
   *
   * private PublicKey getKekPublicKey() throws NoSuchAlgorithmException, IOException,
   * InvalidKeySpecException { return null; }
   *
   * private PrivateKey unwrapPrivateKey(final KeyProviderBundle kpBundle, final String
   * keyWrapAlias, byte[] wrappedPkcs8) throws KeyStoreException, NoSuchPaddingException,
   * NoSuchAlgorithmException, UnrecoverableKeyException, InvalidAlgorithmParameterException,
   * InvalidKeyException {
   *
   * KeyStore keyStore = kpBundle.getKeyStore(); Provider p11Provider = kpBundle.getProvider();
   * PrivateKey kekPriv = (PrivateKey) keyStore.getKey(keyWrapAlias, kpBundle.getKsPassword());
   *
   * var oaep = new OAEPParameterSpec("SHA-1", "MGF1", MGF1ParameterSpec.SHA1,
   * PSource.PSpecified.DEFAULT); Cipher unwrapC =
   * Cipher.getInstance("RSA/ECB/OAEPWithSHA-1AndMGF1Padding", p11Provider);
   * unwrapC.init(Cipher.UNWRAP_MODE, kekPriv, oaep); return (PrivateKey)
   * unwrapC.unwrap(wrappedPkcs8, "EC", Cipher.PRIVATE_KEY); }
   */



  private void validateNonNull(final Object... param) throws ServiceRequestException {
    for (final Object p : param) {
      if (Objects.isNull(p)) {
        throw new ServiceRequestException("Illegal null input parameter");
      }
    }
  }

}
