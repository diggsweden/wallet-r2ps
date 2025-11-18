package se.digg.wallet.r2ps.domain.exception;

public class WalletInternalServerException extends RuntimeException {
  public WalletInternalServerException(String template, Object... args) {
    super(String.format(template, args));
  }
}
