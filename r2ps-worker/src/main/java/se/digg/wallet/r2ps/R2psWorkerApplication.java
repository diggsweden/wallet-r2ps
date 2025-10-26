// SPDX-FileCopyrightText: 2025 diggsweden/wallet-r2ps
//
// SPDX-License-Identifier: EUPL-1.2

package se.digg.wallet.r2ps;

import org.springframework.boot.SpringApplication;
import org.springframework.boot.autoconfigure.SpringBootApplication;
import org.bouncycastle.jce.provider.BouncyCastleProvider;

import java.security.Security;

@SpringBootApplication
public class R2psWorkerApplication {

  static {
    Security.addProvider(new BouncyCastleProvider());
  }

  public static void main(String[] args) {
    SpringApplication.run(R2psWorkerApplication.class, args);
  }

}
