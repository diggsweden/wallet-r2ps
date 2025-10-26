// SPDX-FileCopyrightText: 2025 diggsweden/wallet-r2ps
//
// SPDX-License-Identifier: EUPL-1.2

package se.digg.wallet.r2ps;

import org.springframework.boot.SpringApplication;

public class TestDemoApplication {

  public static void main(String[] args) {
    SpringApplication.from(DemoApplication::main).with(TestcontainersConfiguration.class).run(args);
  }

}
