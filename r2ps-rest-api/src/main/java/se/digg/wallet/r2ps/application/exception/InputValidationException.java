// SPDX-FileCopyrightText: 2025 diggsweden/wallet-r2ps
//
// SPDX-License-Identifier: EUPL-1.2

package se.digg.wallet.r2ps.application.exception;

import java.util.Objects;

public class InputValidationException extends RuntimeException {

  public enum Error {
    INVALID_USER_ID("UserID is invalid");

    private String description;

    Error(String description) {
      this.description = description;
    }

    public String description() {
      return description;
    }
  }

  private final Error title;
  private final String detail;

  public InputValidationException(Error title, String detail) {
    this.title = Objects.requireNonNull(title);
    this.detail = Objects.requireNonNull(detail);
  }

  public Error title() {
    return title;
  }

  public String detail() {
    return detail;
  }

}
