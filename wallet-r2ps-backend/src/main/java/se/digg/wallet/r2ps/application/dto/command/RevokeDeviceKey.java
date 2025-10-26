package se.digg.wallet.r2ps.application.dto.command;

import java.util.UUID;

public record RevokeDeviceKey(String deviceId, CommandMetadata metadata) implements Command  {
}
