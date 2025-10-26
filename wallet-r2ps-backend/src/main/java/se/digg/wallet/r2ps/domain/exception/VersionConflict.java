package se.digg.wallet.r2ps.domain.exception;

public class VersionConflict extends RuntimeException {
  public VersionConflict(String template, Object... args) {
    super(String.format(template, args));
  }
}
