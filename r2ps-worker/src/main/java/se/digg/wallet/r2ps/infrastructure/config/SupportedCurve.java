package se.digg.wallet.r2ps.infrastructure.config;

import java.security.NoSuchAlgorithmException;
import java.util.Arrays;
import java.util.List;

public enum SupportedCurve {

  P256("P-256", "secp256r1"),
  P384("P-384", "secp384r1"),
  P521("P-521", "secp521r1");

  private String id;
  private String jcaName;

  private SupportedCurve(String id, String jcaName) {
    this.id = id;
    this.jcaName = jcaName;
  }

  public static SupportedCurve fromId(final String id) throws NoSuchAlgorithmException {
    return Arrays.stream(values())
        .filter(v -> v.getId().equals(id))
        .findFirst()
        .orElseThrow(() -> new NoSuchAlgorithmException("Unsupported curve: " + id));
  }

  public static List<String> toIdList() {
    return Arrays.stream(values())
        .map(SupportedCurve::getId)
        .toList();
  }

  public String getId() {
    return this.id;
  }

  public String getJcaName() {
    return this.jcaName;
  }
}
