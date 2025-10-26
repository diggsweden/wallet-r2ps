// SPDX-FileCopyrightText: 2025 diggsweden/wallet-r2ps
//
// SPDX-License-Identifier: EUPL-1.2

package se.digg.wallet.r2ps.infrastructure.adapter.in.web;

import org.springframework.web.bind.annotation.RequestBody;
import se.digg.wallet.r2ps.application.dto.command.AddDeviceKey;
import se.digg.wallet.r2ps.application.dto.command.CommandMetadata;
import se.digg.wallet.r2ps.application.dto.command.RegisterServerWallet;

import java.net.URI;
import java.time.Instant;
import java.util.Optional;
import java.util.UUID;

import org.springframework.http.ResponseEntity;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PathVariable;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RestController;
import se.digg.wallet.r2ps.application.port.in.RegisterWalletUseCase;
import se.digg.wallet.r2ps.domain.model.aggregate.ServerWallet;
import se.digg.wallet.r2ps.domain.event.Event;

@RequestMapping("/wallet")
@RestController
public class RegisterWalletController {

  private final RegisterWalletUseCase registerWalletUseCase;

  public RegisterWalletController(RegisterWalletUseCase registerWalletUseCase) {
    this.registerWalletUseCase = registerWalletUseCase;
  }

  @GetMapping("/{walletId}")
  public ServerWallet wallet(@PathVariable UUID walletId) {
    return registerWalletUseCase.getWallet(walletId);
  }

  @PostMapping("")
  public ResponseEntity<Event> registerServerWallet() {
    UUID commandId = UUID.randomUUID();
    UUID walletId = UUID.randomUUID();
    Event event = registerWalletUseCase.registerWallet(new RegisterServerWallet(new CommandMetadata(commandId, walletId, RegisterServerWallet.class.getSimpleName(), Instant.now(), Optional.empty(), Optional.empty())));

    // TODO inte create egentligen, mer en vilja om create.....till event är klart
    // detta ska skickas till command topic för att vi ska kunna skala horisontellt oberoende av
    // postgres begränsningar...

    // TODO server host configurable base url
    URI createdUri = URI.create(String.format("http://localhost:8090/rhsm-bff/wallet/%s", walletId.toString()));
    return ResponseEntity.created(createdUri).build();
  }
/*
  @PostMapping("/{walletId}/device")
  public ResponseEntity<Event> addDevice(@RequestBody AddDeviceKeyDto addDeviceKey) {
    UUID commandId = UUID.randomUUID();

    Event event = registerWalletUseCase.registerWallet(new RegisterServerWallet(new CommandMetadata(commandId, walletId, RegisterServerWallet.class.getSimpleName(), Instant.now(), Optional.empty(), Optional.empty())));

    // TODO inte create egentligen, mer en vilja om create.....till event är klart
    // detta ska skickas till command topic för att vi ska kunna skala horisontellt oberoende av
    // postgres begränsningar...

    // TODO server host configurable base url
    URI createdUri = URI.create(String.format("http://localhost:8090/rhsm-bff/wallet/%s", walletId.toString()));
    return ResponseEntity.created(createdUri).build();
  }

 */

}
