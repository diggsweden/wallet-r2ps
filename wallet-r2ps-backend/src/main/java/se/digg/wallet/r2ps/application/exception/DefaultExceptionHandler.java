// SPDX-FileCopyrightText: 2025 diggsweden/wallet-r2ps
//
// SPDX-License-Identifier: EUPL-1.2

package se.digg.wallet.r2ps.application.exception;

import se.digg.wallet.r2ps.application.dto.BadRequestDto;
import jakarta.servlet.http.HttpServletRequest;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.http.HttpStatus;
import org.springframework.http.ResponseEntity;
import org.springframework.web.bind.annotation.ControllerAdvice;
import org.springframework.web.bind.annotation.ExceptionHandler;
import org.springframework.web.bind.annotation.ResponseStatus;
import se.digg.wallet.r2ps.domain.exception.DeviceAlreadyExistsException;
import se.digg.wallet.r2ps.domain.exception.DeviceNotFoundException;
import se.digg.wallet.r2ps.domain.exception.HsmKeyAlreadyExistsException;
import se.digg.wallet.r2ps.domain.exception.HsmKeyNotFoundException;
import se.digg.wallet.r2ps.domain.exception.VersionConflict;
import se.digg.wallet.r2ps.domain.exception.WalletAlreadyExistsException;
import se.digg.wallet.r2ps.domain.exception.WalletNotFoundException;

@ControllerAdvice
public class DefaultExceptionHandler {

  private static Logger LOGGER = LoggerFactory.getLogger(DefaultExceptionHandler.class);

  @Autowired
  private HttpServletRequest httpServletRequest;

  @ResponseStatus(HttpStatus.INTERNAL_SERVER_ERROR)
  @ExceptionHandler(Throwable.class)
  public void handleAnyException(Throwable e) {
    LOGGER.warn("Uncaught exception, responding with 500", e);
  }

  @ResponseStatus(HttpStatus.NOT_FOUND)
  @ExceptionHandler(WalletNotFoundException.class)
  public void handleWalletNotFoundException() {}

  @ResponseStatus(HttpStatus.CONFLICT)
  @ExceptionHandler(WalletAlreadyExistsException.class)
  public void handleWalletAlreadyExistsException() {}

  @ResponseStatus(HttpStatus.CONFLICT)
  @ExceptionHandler(VersionConflict.class)
  public void handleVersionConflict() {}


  @ResponseStatus(HttpStatus.NOT_FOUND)
  @ExceptionHandler(HsmKeyNotFoundException.class)
  public void handleHsmKeyNotFoundException() {}

  @ResponseStatus(HttpStatus.CONFLICT)
  @ExceptionHandler(HsmKeyAlreadyExistsException.class)
  public void handleHsmKeyAlreadyExistsException() {}

  @ResponseStatus(HttpStatus.NOT_FOUND)
  @ExceptionHandler(DeviceNotFoundException.class)
  public void handleDeviceNotFoundException() {}


  @ResponseStatus(HttpStatus.CONFLICT)
  @ExceptionHandler(DeviceAlreadyExistsException.class)
  public void handleDDeviceAlreadyExistsException() {}

  @ExceptionHandler(InputValidationException.class)
  public ResponseEntity<BadRequestDto> handleInputValidationException2(InputValidationException e) {
    var instance = httpServletRequest.getServletPath();
    var body = new BadRequestDto(
        null,
        e.title().description(),
        400,
        e.detail(),
        instance);
    return ResponseEntity.badRequest().body(body);
  }
}
