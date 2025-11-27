package se.digg.wallet.r2ps.domain.command;

import java.security.PublicKey;

public record AddDeviceKey(PublicKey devicePublicKey, CommandMetadata metadata) implements Command {
}
