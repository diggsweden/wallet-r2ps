package se.digg.wallet.r2ps.domain.exception;

public class DeviceNotFoundException extends RuntimeException {
  public DeviceNotFoundException(String template, Object... args) {
    super(String.format(template, args));

  }
}
