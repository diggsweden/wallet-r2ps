package se.digg.wallet.r2ps.domain.exception;

public class WalletAlreadyExistsException extends RuntimeException {
  public WalletAlreadyExistsException(String template, Object... args) {
    super(String.format(template, args));
  }
}
