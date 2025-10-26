package se.digg.wallet.r2ps.application.dto.command;

public record DeleteHsmKey(String keyId, CommandMetadata metadata) implements Command  {
}
