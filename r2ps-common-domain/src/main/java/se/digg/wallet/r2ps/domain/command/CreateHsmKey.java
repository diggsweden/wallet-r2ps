package se.digg.wallet.r2ps.domain.command;

import java.security.PublicKey;
import java.time.Instant;

// TODO check correct attributes
public record CreateHsmKey(String curveName, Instant creationTime, PublicKey publicKey, CommandMetadata metadata) implements Command {
}
