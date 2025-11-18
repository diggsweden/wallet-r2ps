package se.digg.wallet.r2ps.domain.command;

public record RevokeDeviceKey(String deviceId, CommandMetadata metadata) implements Command  {
}
