package se.digg.wallet.r2ps.infrastructure.config;

import se.digg.wallet.r2ps.infrastructure.frdemo.KeyStoreStrategy;

import java.io.File;
import java.security.KeyPairGenerator;
import java.security.KeyStore;
import java.security.Provider;

public class KeyProviderBundle {
  private String curve;
  private Provider provider;
  private KeyStore keyStore;
  private KeyPairGenerator keyPairGenerator;
  private char[] ksPassword;
  private File ksLocation;
  KeyStoreStrategy keyStoreStrategy;

  public KeyProviderBundle(String curve, Provider provider, KeyStore keyStore,
      KeyPairGenerator keyPairGenerator, char[] ksPassword, File ksLocation,
      KeyStoreStrategy keyStoreStrategy) {
    this.curve = curve;
    this.provider = provider;
    this.keyStore = keyStore;
    this.keyPairGenerator = keyPairGenerator;
    this.ksPassword = ksPassword;
    this.ksLocation = ksLocation;
    this.keyStoreStrategy = keyStoreStrategy;
  }

  public KeyProviderBundle() {}

  public static KeyProviderBundleBuilder builder() {
    return new KeyProviderBundleBuilder();
  }

  public String getCurve() {
    return this.curve;
  }

  public Provider getProvider() {
    return this.provider;
  }

  public KeyStore getKeyStore() {
    return this.keyStore;
  }

  public KeyPairGenerator getKeyPairGenerator() {
    return this.keyPairGenerator;
  }

  public char[] getKsPassword() {
    return this.ksPassword;
  }

  public File getKsLocation() {
    return this.ksLocation;
  }

  public KeyStoreStrategy getKeyStoreStrategy() {
    return this.keyStoreStrategy;
  }

  public void setCurve(String curve) {
    this.curve = curve;
  }

  public void setProvider(Provider provider) {
    this.provider = provider;
  }

  public void setKeyStore(KeyStore keyStore) {
    this.keyStore = keyStore;
  }

  public void setKeyPairGenerator(KeyPairGenerator keyPairGenerator) {
    this.keyPairGenerator = keyPairGenerator;
  }

  public void setKsPassword(char[] ksPassword) {
    this.ksPassword = ksPassword;
  }

  public void setKsLocation(File ksLocation) {
    this.ksLocation = ksLocation;
  }

  public void setKeyStoreStrategy(KeyStoreStrategy keyStoreStrategy) {
    this.keyStoreStrategy = keyStoreStrategy;
  }

  public boolean equals(final Object o) {
    if (o == this)
      return true;
    if (!(o instanceof KeyProviderBundle))
      return false;
    final KeyProviderBundle other = (KeyProviderBundle) o;
    if (!other.canEqual((Object) this))
      return false;
    final Object this$curve = this.getCurve();
    final Object other$curve = other.getCurve();
    if (this$curve == null ? other$curve != null : !this$curve.equals(other$curve))
      return false;
    final Object this$provider = this.getProvider();
    final Object other$provider = other.getProvider();
    if (this$provider == null ? other$provider != null : !this$provider.equals(other$provider))
      return false;
    final Object this$keyStore = this.getKeyStore();
    final Object other$keyStore = other.getKeyStore();
    if (this$keyStore == null ? other$keyStore != null : !this$keyStore.equals(other$keyStore))
      return false;
    final Object this$keyPairGenerator = this.getKeyPairGenerator();
    final Object other$keyPairGenerator = other.getKeyPairGenerator();
    if (this$keyPairGenerator == null ? other$keyPairGenerator != null
        : !this$keyPairGenerator.equals(other$keyPairGenerator))
      return false;
    if (!java.util.Arrays.equals(this.getKsPassword(), other.getKsPassword()))
      return false;
    final Object this$ksLocation = this.getKsLocation();
    final Object other$ksLocation = other.getKsLocation();
    if (this$ksLocation == null ? other$ksLocation != null
        : !this$ksLocation.equals(other$ksLocation))
      return false;
    final Object this$keyStoreStrategy = this.getKeyStoreStrategy();
    final Object other$keyStoreStrategy = other.getKeyStoreStrategy();
    if (this$keyStoreStrategy == null ? other$keyStoreStrategy != null
        : !this$keyStoreStrategy.equals(other$keyStoreStrategy))
      return false;
    return true;
  }

  protected boolean canEqual(final Object other) {
    return other instanceof KeyProviderBundle;
  }

  public int hashCode() {
    final int PRIME = 59;
    int result = 1;
    final Object $curve = this.getCurve();
    result = result * PRIME + ($curve == null ? 43 : $curve.hashCode());
    final Object $provider = this.getProvider();
    result = result * PRIME + ($provider == null ? 43 : $provider.hashCode());
    final Object $keyStore = this.getKeyStore();
    result = result * PRIME + ($keyStore == null ? 43 : $keyStore.hashCode());
    final Object $keyPairGenerator = this.getKeyPairGenerator();
    result = result * PRIME + ($keyPairGenerator == null ? 43 : $keyPairGenerator.hashCode());
    result = result * PRIME + java.util.Arrays.hashCode(this.getKsPassword());
    final Object $ksLocation = this.getKsLocation();
    result = result * PRIME + ($ksLocation == null ? 43 : $ksLocation.hashCode());
    final Object $keyStoreStrategy = this.getKeyStoreStrategy();
    result = result * PRIME + ($keyStoreStrategy == null ? 43 : $keyStoreStrategy.hashCode());
    return result;
  }

  public String toString() {
    return "KeyProviderBundle(curve=" + this.getCurve() + ", provider=" + this.getProvider()
        + ", keyStore=" + this.getKeyStore() + ", keyPairGenerator=" + this.getKeyPairGenerator()
        + ", ksPassword=" + java.util.Arrays.toString(
            this.getKsPassword())
        + ", ksLocation=" + this.getKsLocation() + ", keyStoreStrategy="
        + this.getKeyStoreStrategy() + ")";
  }

  public static class KeyProviderBundleBuilder {
    private String curve;
    private Provider provider;
    private KeyStore keyStore;
    private KeyPairGenerator keyPairGenerator;
    private char[] ksPassword;
    private File ksLocation;
    private KeyStoreStrategy keyStoreStrategy;

    KeyProviderBundleBuilder() {}

    public KeyProviderBundleBuilder curve(String curve) {
      this.curve = curve;
      return this;
    }

    public KeyProviderBundleBuilder provider(Provider provider) {
      this.provider = provider;
      return this;
    }

    public KeyProviderBundleBuilder keyStore(KeyStore keyStore) {
      this.keyStore = keyStore;
      return this;
    }

    public KeyProviderBundleBuilder keyPairGenerator(KeyPairGenerator keyPairGenerator) {
      this.keyPairGenerator = keyPairGenerator;
      return this;
    }

    public KeyProviderBundleBuilder ksPassword(char[] ksPassword) {
      this.ksPassword = ksPassword;
      return this;
    }

    public KeyProviderBundleBuilder ksLocation(File ksLocation) {
      this.ksLocation = ksLocation;
      return this;
    }

    public KeyProviderBundleBuilder keyStoreStrategy(KeyStoreStrategy keyStoreStrategy) {
      this.keyStoreStrategy = keyStoreStrategy;
      return this;
    }

    public KeyProviderBundle build() {
      return new KeyProviderBundle(this.curve, this.provider, this.keyStore, this.keyPairGenerator,
          this.ksPassword, this.ksLocation, this.keyStoreStrategy);
    }

    public String toString() {
      return "KeyProviderBundle.KeyProviderBundleBuilder(curve=" + this.curve + ", provider="
          + this.provider + ", keyStore=" + this.keyStore + ", keyPairGenerator="
          + this.keyPairGenerator + ", ksPassword=" + java.util.Arrays.toString(
              this.ksPassword)
          + ", ksLocation=" + this.ksLocation + ", keyStoreStrategy=" + this.keyStoreStrategy + ")";
    }
  }
}
