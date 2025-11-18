package se.digg.wallet.r2ps.infrastructure.adapter.out.messaging;

import org.springframework.kafka.core.KafkaTemplate;
import org.springframework.stereotype.Service;
import se.digg.wallet.r2ps.application.port.out.R2psRequestMessageSpiPort;
import se.digg.wallet.r2ps.application.port.out.RegisterWalletSpiPort;
import se.digg.wallet.r2ps.domain.command.Command;
import se.digg.wallet.r2ps.infrastructure.adapter.dto.R2psRequestDto;

@Service
public class RegisterWalletMessageSender implements RegisterWalletSpiPort {

  private static final String DEST_TOPIC = "wallet-commands";

  private final KafkaTemplate<String, Command> kafkaTemplate;

  public RegisterWalletMessageSender(KafkaTemplate<String, Command> kafkaTemplate) {
    this.kafkaTemplate = kafkaTemplate;
  }

  @Override
  public void registerWalletCommand(Command command) {
    kafkaTemplate.send(DEST_TOPIC, command.metadata().walletId().toString(), command);
  }
}
