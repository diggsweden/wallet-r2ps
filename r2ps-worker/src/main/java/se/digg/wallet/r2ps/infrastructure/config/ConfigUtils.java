package se.digg.wallet.r2ps.infrastructure.config;

import org.bouncycastle.util.encoders.Base64;
import se.digg.wallet.r2ps.commons.dto.EncryptOption;
import se.digg.wallet.r2ps.commons.dto.servicetype.ServiceTypeRegistry;

import java.io.File;
import java.nio.charset.StandardCharsets;

/**
 * General common utils
 */
public class ConfigUtils {

  public static File getFile(String path, boolean createDirIfMissing) {
    File file = getFile(path);
    if (file == null) {
      return null;
    }
    if (createDirIfMissing && !file.getParentFile().exists()) {
      file.getParentFile().mkdirs();
    }
    return file;
  }

  public static File getFile(String path) {
    if (!hasText(path)) {
      return null;
    }
    if (path.startsWith("classpath:")) {
      return new File(ConfigUtils.class.getResource("/" + path.substring(10)).getFile());
    }
    if (path.startsWith("file://")) {
      path = path.substring(7);
    }
    if (path.startsWith("/")) {
      return new File(path);
    }
    return new File(System.getProperty("user.dir"), path);
  }

  private static boolean hasText(final String string) {
    return (string != null && !string.trim().isEmpty());
  }

  public static String getServiceUrl(String baseUrl, String contextPath) {
    if (contextPath == null || ("/".equals(contextPath))) {
      return baseUrl;
    }
    return baseUrl + contextPath;
  }



  public static String getDataUrl(String data, String mimeType, boolean base64Encode) {
    if (base64Encode) {
      return getDataUrl(data.getBytes(StandardCharsets.UTF_8), mimeType);
    } else {
      return "data:" + mimeType + "," + data;
    }
  }

  public static String getDataUrl(byte[] data, String mimeType) {
    return "data:" + mimeType + ";base64," + Base64.toBase64String(data);
  }

  public static ServiceTypeRegistry getDemoServiceTypeRegistry() {
    ServiceTypeRegistry serviceTypeRegistry = new ServiceTypeRegistry();
    serviceTypeRegistry.registerServiceType(DemoServiceType.REGISTER_AUTHORIZATION,
        EncryptOption.device);
    return serviceTypeRegistry;
  }

}


