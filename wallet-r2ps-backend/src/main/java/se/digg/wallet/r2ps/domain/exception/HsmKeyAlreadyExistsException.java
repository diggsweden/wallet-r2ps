package se.digg.wallet.r2ps.domain.exception;

public class HsmKeyAlreadyExistsException extends RuntimeException {
  public HsmKeyAlreadyExistsException(String template, Object... args) {
    super(String.format(template, args));

  }
}
