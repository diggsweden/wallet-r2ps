package se.digg.wallet.r2ps.infrastructure.frdemo.impl;

import org.slf4j.Logger;
import se.digg.wallet.r2ps.commons.exception.ServiceRequestException;
import se.digg.wallet.r2ps.infrastructure.config.KeyProviderBundle;
import se.digg.wallet.r2ps.infrastructure.config.SupportedCurve;
import se.digg.wallet.r2ps.infrastructure.frdemo.EcKeyPairRecord;
import se.digg.wallet.r2ps.infrastructure.frdemo.KeyCacheRecord;
import se.digg.wallet.r2ps.infrastructure.frdemo.PrivateKeyWrapper;

import java.io.BufferedReader;
import java.io.File;
import java.io.IOException;
import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.security.KeyStore;
import java.security.KeyStoreException;
import java.security.NoSuchAlgorithmException;
import java.security.PrivateKey;
import java.security.SecureRandom;
import java.security.UnrecoverableKeyException;
import java.security.cert.CertificateException;
import java.util.ArrayList;
import java.util.List;
import java.util.Random;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.locks.ReadWriteLock;
import java.util.concurrent.locks.ReentrantReadWriteLock;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * A wrapper implementation for handling private key operations using external command-line tools,
 * particularly for PKCS#11 HSM (Hardware Security Module) integration.
 * <p>
 * This class implements the {@code PrivateKeyWrapper} interface and provides methods to wrap and
 * unwrap keys using the configured HSM setup.
 * <p>
 * Environment variables required: - {@code PKCS11LIB}: Path to the PKCS#11 library. -
 * {@code PKCS11SLOT}: Slot index for the HSM. - {@code PKCS11PASSWORD}: PIN for the HSM, required
 * by {@code p11wrap} and {@code p11unwrap}.
 * <p>
 * The wrapping operation is performed using the command line {@code pkcs11-tool} from OpenSC,
 * {@code p11tool} from GnuTLS, {@code p11wrap} and {@code p11unwrap} from MasterCard.
 */
public class CLIPrivateKeyWrapper implements PrivateKeyWrapper {

  public static final String PKCS11LIB_ENV = "PKCS11LIB";
  public static final String HSM_SLOT_INDEX_ENV = "PKCS11SLOT";
  public static final String WRAP_ALGORITHM = "rfc5649";
  public static final Random RNG = new SecureRandom();
  private static final Pattern SLOT_LINE =
      Pattern.compile("^Slot\\s+(\\d+)\\s+\\((0x[0-9a-fA-F]+)\\).*");
  private static final Logger log = org.slf4j.LoggerFactory.getLogger(CLIPrivateKeyWrapper.class);

  private final File wrapTempDir;
  private final String wrapKeyAlias;
  private final ReadWriteLock ksLock = new ReentrantReadWriteLock();

  public CLIPrivateKeyWrapper(final File wrapTempDir, final String wrapKeyAlias) {
    this.wrapTempDir = wrapTempDir;
    this.wrapKeyAlias = wrapKeyAlias;
  }

  private static String env(String name) {
    String val = System.getenv(name);
    if (val == null || val.isEmpty()) {
      throw new IllegalStateException("Environment variable " + name + " not set");
    }
    return val;
  }

  @Override
  public byte[] wrapKey(final String keyLabel, final KeyProviderBundle kpBundle)
      throws ServiceRequestException {
    try {
      // Wrap
      File wrapFile = new File(wrapTempDir, keyLabel + ".p11w");
      runCommand(CmdBuilder.builder("p11wrap")
          .addArg("-w", wrapKeyAlias)
          .addArg("-a", WRAP_ALGORITHM)
          .addArg("-i", keyLabel)
          .addArg("-o", wrapFile.getAbsolutePath())
          .addArg("CKA_CLASS=CKO_PRIVATE_KEY")
          .build());
      byte[] wrappedKey = Files.readAllBytes(wrapFile.toPath());
      wrapFile.delete();
      log.debug("Wrapped key:\n{}", new String(wrappedKey, StandardCharsets.UTF_8));
      ksLock.writeLock().lock();
      try {
        kpBundle.getKeyStore().deleteEntry(keyLabel);
      } finally {
        ksLock.writeLock().unlock();
      }
      return wrappedKey;
    } catch (IOException | KeyStoreException e) {
      throw new ServiceRequestException("Error wrapping key: " + e.getMessage(), e);
    }
  }

  @Override
  public void generateKey(final String keyLabel, final KeyProviderBundle kpBundle)
      throws ServiceRequestException {

    try {
      KeyStore keyStore = kpBundle.getKeyStore();
      String slot = getSlotNumber();
      String hsmPin = new String(kpBundle.getKsPassword());
      String id = generateId();

      runCommand(CmdBuilder.builder("bash")
          .addArg("/p11-keygen.sh")
          .addArg("-p", hsmPin)
          .addArg("-s", slot)
          .addArg("-a", keyLabel)
          .addArg("-i", id)
          .addArg("--key-type", "EC:" + SupportedCurve.fromId(kpBundle.getCurve()).getJcaName())
          .addArg("-v", "3652")
          .build());

      runCommand(CmdBuilder.builder("pkcs11-tool")
          .addArg("--module", env(PKCS11LIB_ENV))
          .addArg("--slot-index", env(HSM_SLOT_INDEX_ENV))
          .addArg("--login")
          .addArg("--pin", hsmPin)
          .addArg("--delete-object")
          .addArg("--type", "pubkey")
          .addArg("--label", keyLabel)
          .build());

      try {
        ksLock.writeLock().lock();
        keyStore.load(null, kpBundle.getKsPassword());
      } finally {
        ksLock.writeLock().unlock();
      }
    } catch (
        NoSuchAlgorithmException | IOException | CertificateException e) {
      throw new ServiceRequestException("Unsupported curve: " + kpBundle.getCurve(), e);
    }
  }

  private String generateId() {
    char[] id = new char[18];
    for (int i = 0; i < id.length; i++) {
      id[i] = (char) ('0' + RNG.nextInt(10));
    }
    return new String(id);
  }

  private String getSlotNumber() throws ServiceRequestException {

    final List<String> slotInfoLines = runCommand(CmdBuilder.builder("bash")
        .addArg("/p11-keygen.sh")
        .addArg("--list")
        .build());

    String idx = env(HSM_SLOT_INDEX_ENV);
    for (String raw : slotInfoLines) {
      String s = raw.trim();
      Matcher m = SLOT_LINE.matcher(s);
      if (m.matches() && m.group(1).equals(idx)) {
        return m.group(2);
      }
    }
    throw new ServiceRequestException("Unable to find slot " + idx);
  }

  @Override
  public PrivateKey unwrapKey(EcKeyPairRecord keyPairRecord, final String keyLabel,
      final KeyProviderBundle kpBundle)
      throws ServiceRequestException {
    try {
      KeyStore keyStore = kpBundle.getKeyStore();
      String hsmPin = new String(kpBundle.getKsPassword());
      String privKeyId = getHSMKeyId(keyLabel, hsmPin, "privkey");
      if (privKeyId == null) {
        File wrapFile = new File(wrapTempDir, keyLabel + ".p11w");
        Files.write(wrapFile.toPath(), keyPairRecord.privateKey());
        runCommand(CmdBuilder.builder("p11unwrap")
            .addArg("-f", wrapFile.getAbsolutePath())
            .build());
        wrapFile.delete();
        privKeyId = getHSMKeyId(keyLabel, hsmPin, "privkey");
        if (privKeyId == null) {
          throw new ServiceRequestException("Unable to restore private key for label " + keyLabel);
        }
      }
      String certId = getHSMKeyId(keyLabel, hsmPin, "cert");
      if (certId == null) {
        keyPairRecord.certificate();
        File certFile = new File(wrapTempDir, keyLabel + ".crt");
        Files.write(certFile.toPath(), keyPairRecord.certificate().getEncoded());
        runCommand(CmdBuilder.builder("pkcs11-tool")
            .addArg("--module", env(PKCS11LIB_ENV))
            .addArg("--slot-index", env(HSM_SLOT_INDEX_ENV))
            .addArg("--login")
            .addArg("--pin", new String(kpBundle.getKsPassword()))
            .addArg("--write-object", certFile.getAbsolutePath())
            .addArg("--type", "cert")
            .addArg("--label", keyLabel)
            .addArg("--set-id", privKeyId)
            .build());
        certFile.delete();
      }
      ksLock.writeLock().lock();
      try {
        keyStore.load(null, kpBundle.getKsPassword());
      } finally {
        ksLock.writeLock().unlock();
      }
      if (keyStore.isKeyEntry(keyLabel)) {
        // If the private key already exists. Return it
        log.debug("Found existing private key for label {}", keyLabel);
      } else {
        throw new ServiceRequestException("Unable to restore private key for label " + keyLabel);
      }
      return (PrivateKey) keyStore.getKey(keyLabel, kpBundle.getKsPassword());
    } catch (
        KeyStoreException | UnrecoverableKeyException | NoSuchAlgorithmException | IOException
        | CertificateException e) {
      throw new RuntimeException(e);
    }
  }

  @Override
  public void deleteKeyFromHsm(final KeyCacheRecord keyCacheRecord) throws ServiceRequestException {
    log.debug("Deleting key {} from HSM", keyCacheRecord.alias());
    try {
      KeyStore keyStore = keyCacheRecord.keyStore();
      String hsmPin = new String(keyCacheRecord.pin());
      String label = keyCacheRecord.alias();
      String certId = getHSMKeyId(keyCacheRecord.alias(), hsmPin, "cert");
      String privKeyId =
          certId != null ? certId : getHSMKeyId(keyCacheRecord.alias(), hsmPin, "privkey");
      if (certId != null) {
        runCommand(CmdBuilder.builder("pkcs11-tool")
            .addArg("--module", env(PKCS11LIB_ENV))
            .addArg("--slot-index", env(HSM_SLOT_INDEX_ENV))
            .addArg("--login")
            .addArg("--pin", hsmPin)
            .addArg("--delete-object")
            .addArg("--type", "cert")
            .addArg("--label", label)
            .build());
        log.debug("Successfully deleted certificate for alias {}", keyCacheRecord.alias());
      } else {
        log.debug("No certificate found for alias {}", keyCacheRecord.alias());
      }
      if (privKeyId != null) {
        runCommand(CmdBuilder.builder("pkcs11-tool")
            .addArg("--module", env(PKCS11LIB_ENV))
            .addArg("--slot-index", env(HSM_SLOT_INDEX_ENV))
            .addArg("--login")
            .addArg("--pin", hsmPin)
            .addArg("--delete-object")
            .addArg("--type", "privkey")
            .addArg("--label", label)
            .build());
        log.debug("Successfully deleted private key for alias {}", keyCacheRecord.alias());
      } else {
        log.debug("No private key found for alias {}", keyCacheRecord.alias());
      }
      if (privKeyId == null && certId == null) {
        log.info("No key or certificate found for alias {}", keyCacheRecord.alias());
        return;
      }
      ksLock.writeLock().lock();
      try {
        keyStore.load(null, keyCacheRecord.pin());
      } finally {
        ksLock.writeLock().unlock();
      }
    } catch (CertificateException | IOException | NoSuchAlgorithmException e) {
      throw new ServiceRequestException("Error deleting key from HSM: " + e.getMessage(), e);
    }
  }

  private String getHSMKeyId(final String keyLabel, final String hsmPin, final String type)
      throws ServiceRequestException {
    List<String> result = getKeyInfo(keyLabel, null, type, hsmPin);
    return result.stream()
        .map(String::trim)
        .filter(s -> s.matches("^ID:\\s*\\S+$"))
        .map(s -> s.substring(3).trim())
        .findFirst().orElse(null);
  }

  private String getPrivateKeyURI(final String keyId, final String hsmPin)
      throws ServiceRequestException {
    List<String> result = getKeyInfo(null, keyId, "privkey", hsmPin);
    String uri = result.stream()
        .map(String::trim)
        .filter(s -> s.matches("^uri:\\s*\\S+$"))
        .map(s -> s.substring(4).trim())
        .findFirst().orElse(null);
    if (uri == null) {
      return null;
    }
    final String[] uriComponents = uri.split(";");
    List<String> p11toolUriComponents = new ArrayList<>();
    for (String component : uriComponents) {
      if (!component.startsWith("id=")) {
        p11toolUriComponents.add(component);
        continue;
      }
      p11toolUriComponents.add("id=" + percentExpand(keyId));
    }
    return String.join(";", p11toolUriComponents);
  }

  private String percentExpand(final String keyId) throws ServiceRequestException {
    if (keyId == null || keyId.isBlank() || keyId.length() % 2 != 0) {
      throw new ServiceRequestException("Illegal HSM keyId");
    }
    StringBuilder result = new StringBuilder();
    for (int i = 0; i < keyId.length(); i += 2) {
      result.append('%');
      result.append(keyId, i, i + 2);
    }
    return result.toString();
  }

  private List<String> getKeyInfo(final String keyLabel, final String keyId, final String type,
      final String hsmPin)
      throws ServiceRequestException {
    final CmdBuilder builder = CmdBuilder.builder("pkcs11-tool")
        .addArg("--module", env(PKCS11LIB_ENV))
        .addArg("--slot-index", env(HSM_SLOT_INDEX_ENV))
        .addArg("--login")
        .addArg("--pin", hsmPin)
        .addArg("-O")
        .addArg("--type", type);
    if (keyId != null) {
      builder.addArg("--id", keyId);
    }
    if (keyLabel != null) {
      builder.addArg("--label", keyLabel);
    }
    return runCommand(builder.build());
  }

  private List<String> runCommand(final List<String> cmd) throws ServiceRequestException {
    try {
      log.debug("Running command: {}", String.join(" ", cmd));
      ProcessBuilder processBuilder = new ProcessBuilder(cmd);
      processBuilder.redirectErrorStream(true);
      Process process = processBuilder.start();

      List<String> output = new ArrayList<>();
      try (BufferedReader reader = new BufferedReader(
          new InputStreamReader(process.getInputStream(), StandardCharsets.UTF_8))) {
        for (String line; (line = reader.readLine()) != null;) {
          output.add(line);
        }
      }

      if (!process.waitFor(20, TimeUnit.SECONDS)) {
        process.destroyForcibly();
        throw new ServiceRequestException("Command timed out");
      }

      int exitCode = process.exitValue();
      if (exitCode != 0) {
        log.error("Command execution failed with exit code: {}", exitCode);
        throw new ServiceRequestException(
            "Command failed (exit " + exitCode + "): " + String.join("\n", output));
      }

      log.debug("Command execution result:\n{}", String.join("\n", output));
      return output;
    } catch (InterruptedException e) {
      Thread.currentThread().interrupt();
      throw new ServiceRequestException("Command interrupted", e);
    } catch (IOException e) {
      log.error("Error executing command: {}", e.getMessage(), e);
      throw new ServiceRequestException("Error executing command: " + e.getMessage(), e);
    }
  }

  public static class CmdBuilder {

    private final String command;
    private List<String[]> args;

    public static CmdBuilder builder(final String command) {
      return new CmdBuilder(command);
    }

    public CmdBuilder(final String command) {
      this.command = command;
      this.args = new ArrayList<>();
    }

    public CmdBuilder addArg(final String... arg) {
      if (arg == null || arg.length == 0) {
        throw new IllegalArgumentException("Argument cannot be null or empty");
      }
      this.args.add(arg);
      return this;
    }

    public List<String> build() {
      List<String> cmd = new ArrayList<>();
      cmd.add(command);
      for (String[] arg : args) {
        cmd.add(arg[0]);
        if (arg.length == 2) {
          cmd.add(arg[1]);
        }
        if (arg.length > 2) {
          String[] subArgs = new String[arg.length - 1];
          System.arraycopy(arg, 1, subArgs, 0, subArgs.length);
          cmd.add(String.join(",", subArgs));
        }
      }
      return cmd;
    }

  }

}
