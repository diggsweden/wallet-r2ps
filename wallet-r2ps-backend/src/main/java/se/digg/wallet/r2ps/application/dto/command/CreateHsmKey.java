package se.digg.wallet.r2ps.application.dto.command;

import java.security.PublicKey;
import java.time.Instant;
import java.util.UUID;

// TODO check correct attributes
public record CreateHsmKey(String curveName, Instant creationTime, PublicKey publicKey, CommandMetadata metadata) implements Command {
}
