package se.digg.wallet.r2ps.application.port.out;

import java.util.Optional;
import java.util.UUID;

public interface AuthorizationCodeSpiPort {
  void setAuthorizationCode(UUID walletId, String kid, String authorizationCode);

  Optional<String> getAuthorizationCode(UUID walletId, String kid);
}
