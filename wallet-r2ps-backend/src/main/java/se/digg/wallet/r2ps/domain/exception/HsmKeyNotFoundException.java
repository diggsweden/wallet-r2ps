package se.digg.wallet.r2ps.domain.exception;

public class HsmKeyNotFoundException extends RuntimeException {
  public HsmKeyNotFoundException(String template, Object... args) {
    super(String.format(template, args));

  }
}
