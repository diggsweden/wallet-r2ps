package se.digg.wallet.r2ps.domain.command;

public record DeleteHsmKey(String keyId, CommandMetadata metadata) implements Command  {
}
