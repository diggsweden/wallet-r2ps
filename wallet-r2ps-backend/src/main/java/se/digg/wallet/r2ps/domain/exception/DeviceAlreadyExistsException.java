package se.digg.wallet.r2ps.domain.exception;

public class DeviceAlreadyExistsException extends RuntimeException {
  public DeviceAlreadyExistsException(String template, Object... args) {
    super(String.format(template, args));

  }
}
