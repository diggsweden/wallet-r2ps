package se.digg.wallet.r2ps.application.port.in;

import org.springframework.transaction.annotation.Transactional;
import se.digg.wallet.r2ps.application.dto.command.AddDeviceKey;
import se.digg.wallet.r2ps.application.dto.command.Command;
import se.digg.wallet.r2ps.application.dto.command.CreateHsmKey;
import se.digg.wallet.r2ps.application.dto.command.DeleteHsmKey;
import se.digg.wallet.r2ps.application.dto.command.RegisterServerWallet;
import se.digg.wallet.r2ps.application.dto.command.RevokeDeviceKey;
import se.digg.wallet.r2ps.application.dto.command.RevokeServerWallet;
import se.digg.wallet.r2ps.domain.event.DeviceKeyAdded;
import se.digg.wallet.r2ps.domain.event.DeviceKeyRevoked;
import se.digg.wallet.r2ps.domain.event.HsmKeyCreated;
import se.digg.wallet.r2ps.domain.event.HsmKeyDeleted;
import se.digg.wallet.r2ps.domain.event.ServerWalletRevoked;
import se.digg.wallet.r2ps.domain.model.aggregate.DeviceKey;
import se.digg.wallet.r2ps.domain.model.aggregate.ServerWallet;
import se.digg.wallet.r2ps.domain.event.Event;
import se.digg.wallet.r2ps.domain.event.ServerWalletRegistered;

import java.util.UUID;

public interface RegisterWalletUseCase {
    ServerWalletRegistered registerWallet(RegisterServerWallet createWallet);
    ServerWalletRevoked revokeWallet(RevokeServerWallet revokeServerWallet);

    DeviceKeyAdded addDeviceKey(AddDeviceKey addDeviceKey);
    DeviceKeyRevoked revokeDeviceKey(RevokeDeviceKey revokeDeviceKey);

    HsmKeyCreated createHsmKey(CreateHsmKey createHsmKey);
    HsmKeyDeleted deleteHsmKey(DeleteHsmKey deleteHsmKey);

    ServerWallet getWallet(UUID walletId);
    Event registerWalletCommand(Command command);
}
