package se.digg.wallet.r2ps.domain.exception;

public class WalletNotFoundException extends RuntimeException {
  public WalletNotFoundException(String template, Object... args) {
    super(String.format(template, args));
  }
}
