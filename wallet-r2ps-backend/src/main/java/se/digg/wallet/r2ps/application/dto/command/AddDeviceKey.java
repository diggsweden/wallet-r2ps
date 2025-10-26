package se.digg.wallet.r2ps.application.dto.command;

import java.security.PublicKey;
import java.util.UUID;

public record AddDeviceKey(String deviceId, PublicKey devicePublicKey, CommandMetadata metadata) implements Command {
}
