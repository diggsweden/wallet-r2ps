package se.digg.wallet.r2ps.infrastructure.frdemo;

public enum KeyStoreStrategy {
  /** Keys are permanently stored in the key store as objects */
  objects,
  /** Keys are wrapped and exported after creation and never stored as objects */
  wrapped
}
