package se.digg.wallet.r2ps.application.port.out;

import se.digg.wallet.r2ps.commons.pake.opaque.PakeSessionRegistry;
import se.digg.wallet.r2ps.server.pake.opaque.ServerPakeRecord;

public interface PakeSessionRegistrySpiPort extends PakeSessionRegistry<ServerPakeRecord> {
}
