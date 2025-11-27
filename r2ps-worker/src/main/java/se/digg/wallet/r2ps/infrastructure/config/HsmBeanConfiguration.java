package se.digg.wallet.r2ps.infrastructure.config;

import org.slf4j.Logger;
import org.springframework.context.annotation.Bean;
import org.springframework.context.annotation.Configuration;
import se.digg.wallet.r2ps.infrastructure.frdemo.EcKeyPairRecordRegistry;
import se.digg.wallet.r2ps.infrastructure.frdemo.GenericHSMServiceHandler;
import se.digg.wallet.r2ps.infrastructure.frdemo.HsmPrivateKeyCache;
import se.digg.wallet.r2ps.infrastructure.frdemo.KeyStoreStrategy;
import se.digg.wallet.r2ps.infrastructure.frdemo.PrivateKeyWrapper;
import se.digg.wallet.r2ps.infrastructure.frdemo.impl.CLIPrivateKeyWrapper;
import se.digg.wallet.r2ps.infrastructure.frdemo.impl.GenericEcKeyPairRecordRegistry;

import java.io.File;
import java.io.FileInputStream;
import java.io.IOException;
import java.nio.file.Files;
import java.security.InvalidAlgorithmParameterException;
import java.security.KeyPairGenerator;
import java.security.KeyStore;
import java.security.KeyStoreException;
import java.security.NoSuchAlgorithmException;
import java.security.Provider;
import java.security.Security;
import java.security.cert.CertificateException;
import java.security.spec.ECGenParameterSpec;
import java.time.Duration;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;

@Configuration
public class HsmBeanConfiguration {

  private static final Logger log = org.slf4j.LoggerFactory.getLogger(HsmBeanConfiguration.class);

  @Bean
  public Map<String, KeyProviderBundle> provider(R2psBaseServerProperties props)
      throws NoSuchAlgorithmException, InvalidAlgorithmParameterException, KeyStoreException,
      CertificateException, IOException {
    Map<String, KeyProviderBundle> keyProviderBundles = new HashMap<>();
    final R2psBaseServerProperties.HSMConfigurationProperties walletKeyConf = props.getWalletKeys();
    if (walletKeyConf != null) {
      final String keyWrapAlias = walletKeyConf.getKeyWrapAlias();
      KeyStoreStrategy keyStoreStrategy = keyWrapAlias != null && !keyWrapAlias.isBlank()
          ? KeyStoreStrategy.wrapped
          : KeyStoreStrategy.objects;
      final List<R2psBaseServerProperties.PKCS11ConfigFileProperties> p11ConfigList =
          Optional.ofNullable(walletKeyConf.getPkcs11Config()).orElse(new ArrayList<>());
      if (!p11ConfigList.isEmpty()) {
        // HSM configuration. Return a Map of P11 configs for each supported curve
        for (R2psBaseServerProperties.PKCS11ConfigFileProperties p11Config : p11ConfigList) {
          log.info(
              "Configuring HSM provider, KeyStore and KeyPairGenerator for curve: {}, using configuration at: {} ",
              p11Config.getCurve(), p11Config.getLocation());
          final File hsmConfigFile = ConfigUtils.getFile(p11Config.getLocation());
          log.info("HSM config file exists: {}", hsmConfigFile.exists());
          log.debug("HSM config file content{}", Files.readString(hsmConfigFile.toPath()));
          Provider p11Provider =
              Security.getProvider("SunPKCS11").configure(hsmConfigFile.getAbsolutePath());

          p11Provider.configure(hsmConfigFile.getAbsolutePath());
          log.info("Configured HSM provider: {}", p11Provider.getName());
          keyProviderBundles.put(p11Config.getCurve().getId(), KeyProviderBundle.builder()
              .curve(p11Config.getCurve().getId())
              .provider(p11Provider)
              .keyStore(
                  getKeyStore("PKCS11", p11Provider, null, walletKeyConf.getKeystorePassword()))
              .keyPairGenerator(getKeyPairGenerator(p11Provider, p11Config.getCurve().getJcaName()))
              .ksPassword(walletKeyConf.getKeystorePassword().toCharArray())
              .keyStoreStrategy(keyStoreStrategy)
              .build());
        }
        return keyProviderBundles;
      }
    }
    File keyStoreFile = ConfigUtils.getFile(walletKeyConf.keystoreFileLocation);
    Provider bcProvider = Security.getProvider("BC");
    KeyStore keyStore = getKeyStore("JKS", null, keyStoreFile, walletKeyConf.keystorePassword);
    for (SupportedCurve curve : SupportedCurve.values()) {
      keyProviderBundles.put(curve.getId(), KeyProviderBundle.builder()
          .curve(curve.getId())
          .provider(Security.getProvider("BC"))
          .keyStore(keyStore)
          .keyPairGenerator(getKeyPairGenerator(bcProvider, curve.getJcaName()))
          .ksLocation(keyStoreFile)
          .ksPassword(walletKeyConf.keystorePassword.toCharArray())
          .keyStoreStrategy(KeyStoreStrategy.objects)
          .build());
    }
    return keyProviderBundles;
  }

  private KeyStore getKeyStore(final String type, final Provider provider, final File keyStoreFile,
      final String keystorePassword)
      throws KeyStoreException, IOException, CertificateException, NoSuchAlgorithmException {

    log.info("Creating HSM KeyStore of type {} with provider: {}", type,
        provider == null ? "null" : provider.getName());
    KeyStore keyStore = provider == null
        ? KeyStore.getInstance(type)
        : KeyStore.getInstance(type, provider);
    if (keyStoreFile != null) {
      if (keyStoreFile.exists()) {
        keyStore.load(new FileInputStream(keyStoreFile), keystorePassword.toCharArray());
      } else {
        keyStoreFile.getParentFile().mkdirs();
        keyStore.load(null, keystorePassword.toCharArray());
      }
    } else {
      keyStore.load(null, keystorePassword.toCharArray());
    }
    return keyStore;
  }

  private KeyPairGenerator getKeyPairGenerator(final Provider provider, final String curve)
      throws NoSuchAlgorithmException, InvalidAlgorithmParameterException {
    KeyPairGenerator kpg = KeyPairGenerator.getInstance("EC", provider);
    kpg.initialize(new ECGenParameterSpec(curve));
    return kpg;
  }

  @Bean
  public HsmPrivateKeyCache hsmPrivateKeyCache(PrivateKeyWrapper privateKeyWrapper,
      R2psBaseServerProperties props) {
    Duration hsmKeyDuration =
        Optional.ofNullable(props.getWalletKeys().getHsmKeyRetensionDuration())
            .orElse(Duration.ofMinutes(5));
    return new HsmPrivateKeyCache(hsmKeyDuration, 10000, privateKeyWrapper);
  }

  @Bean
  EcKeyPairRecordRegistry ecKeyPairRecordRegistry(Map<String, KeyProviderBundle> providerBundles,
      R2psBaseServerProperties props, HsmPrivateKeyCache hsmPrivateKeyCache,
      PrivateKeyWrapper privateKeyWrapper) {
    File configDir = ConfigUtils.getFile(props.getConfigLocation());
    return new GenericEcKeyPairRecordRegistry(providerBundles, configDir, hsmPrivateKeyCache,
        props.getWalletKeys().getKeyWrapAlias(), privateKeyWrapper);
  }

  @Bean
  public GenericHSMServiceHandler genericHSMServiceHandler(
      Map<String, KeyProviderBundle> providerBundles,
      EcKeyPairRecordRegistry ecKeyPairRecordRegistry) {
    return new GenericHSMServiceHandler(SupportedCurve.toIdList(), List.of("hsm"),
        ecKeyPairRecordRegistry,
        providerBundles);
  }

  @Bean
  PrivateKeyWrapper privateKeyWrapper(R2psBaseServerProperties props) {
    File wrapDir = new File(ConfigUtils.getFile(props.getConfigLocation()), "wrap-temp");
    wrapDir.mkdirs();
    String keyWrapAlias = props.getWalletKeys().getKeyWrapAlias();
    return new CLIPrivateKeyWrapper(wrapDir, keyWrapAlias);
  }
}


