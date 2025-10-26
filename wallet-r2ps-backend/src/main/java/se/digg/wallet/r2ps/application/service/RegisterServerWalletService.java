package se.digg.wallet.r2ps.application.service;

import org.springframework.stereotype.Service;
import org.springframework.transaction.annotation.Transactional;
import se.digg.wallet.r2ps.application.dto.command.AddDeviceKey;
import se.digg.wallet.r2ps.application.dto.command.Command;
import se.digg.wallet.r2ps.application.dto.command.CreateHsmKey;
import se.digg.wallet.r2ps.application.dto.command.DeleteHsmKey;
import se.digg.wallet.r2ps.application.dto.command.RegisterServerWallet;
import se.digg.wallet.r2ps.application.dto.command.RevokeDeviceKey;
import se.digg.wallet.r2ps.application.dto.command.RevokeServerWallet;
import se.digg.wallet.r2ps.application.mapper.CommandToEventsMapper;
import se.digg.wallet.r2ps.application.port.in.RegisterWalletUseCase;
import se.digg.wallet.r2ps.application.port.out.EventPublisherSpiPort;
import se.digg.wallet.r2ps.application.port.out.ServerWalletRepository;
import se.digg.wallet.r2ps.application.service.aggregate.ServerWalletEventApplier;
import se.digg.wallet.r2ps.domain.event.DeviceKeyAdded;
import se.digg.wallet.r2ps.domain.event.DeviceKeyRevoked;
import se.digg.wallet.r2ps.domain.event.HsmKeyCreated;
import se.digg.wallet.r2ps.domain.event.HsmKeyDeleted;
import se.digg.wallet.r2ps.domain.event.ServerWalletRevoked;
import se.digg.wallet.r2ps.domain.exception.WalletNotFoundException;
import se.digg.wallet.r2ps.domain.model.aggregate.ServerWallet;
import se.digg.wallet.r2ps.domain.event.Event;
import se.digg.wallet.r2ps.domain.event.ServerWalletRegistered;
import java.util.Optional;
import java.util.UUID;

@Service
@Transactional
public class RegisterServerWalletService implements RegisterWalletUseCase {
/*
Chose to use the transactional outbox pattern instead of event sourcing because it can be
assumed that changes will be relatively sparse. That is, a regular relational database can be
expected to be able to process the changes in worker nodes for a partition.
*/

  private final ServerWalletRepository walletRepository;
  private final EventPublisherSpiPort eventPublisher;
  private final CommandToEventsMapper command2EventsMapper;
  private final ServerWalletEventApplier aggregateServerWallet;

  public RegisterServerWalletService(ServerWalletRepository walletRepository,
      EventPublisherSpiPort eventPublisher, CommandToEventsMapper command2EventsMapper,
      ServerWalletEventApplier aggregateServerWallet) {
    this.walletRepository = walletRepository;
    this.eventPublisher = eventPublisher;
    this.command2EventsMapper = command2EventsMapper;
    this.aggregateServerWallet = aggregateServerWallet;
  }

  @Override
  public ServerWallet getWallet(UUID walletId) {
    return walletRepository.findById(walletId).orElseThrow(() -> new WalletNotFoundException(walletId.toString()));
  }

  @Override
  public ServerWalletRegistered registerWallet(RegisterServerWallet createWallet) {
     Event e = registerWalletCommand(createWallet);
     return (ServerWalletRegistered) e;
  }

  @Override
  public ServerWalletRevoked revokeWallet(RevokeServerWallet revokeServerWallet) {
    Event e = registerWalletCommand(revokeServerWallet);
    return (ServerWalletRevoked) e;
  }

  @Override
  public DeviceKeyAdded addDeviceKey(AddDeviceKey addDeviceKey) {
    Event e = registerWalletCommand(addDeviceKey);
    return (DeviceKeyAdded) e;  }

  @Override
  public DeviceKeyRevoked revokeDeviceKey(RevokeDeviceKey revokeDeviceKey) {
    Event e = registerWalletCommand(revokeDeviceKey);
    return (DeviceKeyRevoked) e;
  }

  @Override
  public HsmKeyCreated createHsmKey(CreateHsmKey createHsmKey) {
    Event e = registerWalletCommand(createHsmKey);
    return (HsmKeyCreated) e;
  }

  @Override
  public HsmKeyDeleted deleteHsmKey(DeleteHsmKey deleteHsmKey) {
    Event e = registerWalletCommand(deleteHsmKey);
    return (HsmKeyDeleted) e;
  }

  @Transactional
  @Override
  public Event registerWalletCommand(Command command) {
    // Get previous aggregate state
    Optional<ServerWallet> previousVersion = walletRepository.findById(command.metadata().walletId());

    // Create and save aggregate
    Event event = command2EventsMapper.handleCommand(command, previousVersion);
    ServerWallet nextVersion = aggregateServerWallet.apply(previousVersion, event);

    walletRepository.save(nextVersion);

    // save to event outbox table in the same transaction
    eventPublisher.publish(event);

    return event;
  }

}
