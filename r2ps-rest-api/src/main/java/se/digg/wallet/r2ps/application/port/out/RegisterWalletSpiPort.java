package se.digg.wallet.r2ps.application.port.out;


import se.digg.wallet.r2ps.domain.command.Command;

public interface RegisterWalletSpiPort {
  void registerWalletCommand(Command command);
}
