package se.digg.wallet.r2ps.domain.command;

import java.security.PublicKey;
import java.util.Optional;

public record RegisterServerWallet(Optional<PublicKey> devicePublicKey, CommandMetadata metadata) implements Command  {
}
