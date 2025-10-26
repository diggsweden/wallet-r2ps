package se.digg.wallet.r2ps.application.dto.command;

import java.util.UUID;

public record RevokeServerWallet(CommandMetadata metadata) implements Command  {
}
