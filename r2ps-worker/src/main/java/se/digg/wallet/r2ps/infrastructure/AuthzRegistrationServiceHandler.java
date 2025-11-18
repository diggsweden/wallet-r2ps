package se.digg.wallet.r2ps.infrastructure;

import lombok.extern.slf4j.Slf4j;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import se.digg.wallet.r2ps.commons.dto.ErrorCode;
import se.digg.wallet.r2ps.commons.dto.ServiceRequest;
import se.digg.wallet.r2ps.commons.dto.payload.ByteArrayPayload;
import se.digg.wallet.r2ps.commons.dto.payload.ExchangePayload;
import se.digg.wallet.r2ps.commons.dto.payload.StringPayload;
import se.digg.wallet.r2ps.commons.dto.servicetype.ServiceType;
import se.digg.wallet.r2ps.commons.exception.ServiceRequestHandlingException;
import se.digg.wallet.r2ps.server.pake.opaque.ServerPakeRecord;
import se.digg.wallet.r2ps.server.service.ClientPublicKeyRecord;
import se.digg.wallet.r2ps.server.service.ClientPublicKeyRegistry;
import se.digg.wallet.r2ps.server.service.servicehandlers.ServiceTypeHandler;

import java.io.IOException;
import java.util.Optional;

public class AuthzRegistrationServiceHandler implements ServiceTypeHandler {

  private static final Logger log = LoggerFactory.getLogger(AuthzRegistrationServiceHandler.class);

  private ClientPublicKeyRegistry clientPublicKeyRegistry;

  public AuthzRegistrationServiceHandler(ClientPublicKeyRegistry clientPublicKeyRegistry) {
    this.clientPublicKeyRegistry = clientPublicKeyRegistry;
  }

  @Override
  public boolean supports(final ServiceType serviceType, final String context) {
    return "register-authorization".equals(serviceType.id());
  }

  @Override
  public ExchangePayload<?> processServiceRequest(final ServiceRequest serviceRequest,
      final ServerPakeRecord pakeSession,
      final byte[] decryptedPayload, final ClientPublicKeyRecord clientPublicKeyRecord,
      final ServiceType serviceType) throws ServiceRequestHandlingException {

    try {
      log.debug("Handling session request {} for context {}", serviceType.id(),
          serviceRequest.getContext());
      final String kid = Optional.ofNullable(serviceRequest.getKid())
          .orElseThrow(() -> new ServiceRequestHandlingException("No KeyID in request",
              ErrorCode.ILLEGAL_REQUEST_DATA));
      final String clientId = Optional.ofNullable(serviceRequest.getClientID())
          .orElseThrow(() -> new ServiceRequestHandlingException("No client ID in request",
              ErrorCode.ILLEGAL_REQUEST_DATA));
      byte[] authzCode = new ByteArrayPayload().deserialize(decryptedPayload).getByteArrayValue();
      clientPublicKeyRegistry.setAuthorizationCode(clientId, kid, authzCode);
      return new StringPayload("OK");
    } catch (NullPointerException | IOException e) {
      throw new ServiceRequestHandlingException(
          String.format("Unable to process session request - %s", e.getMessage()),
          ErrorCode.SERVER_ERROR);
    }
  }

}
