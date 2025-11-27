package se.digg.wallet.r2ps.infrastructure.frdemo;

import lombok.extern.slf4j.Slf4j;
import se.digg.wallet.r2ps.commons.dto.ErrorCode;
import se.digg.wallet.r2ps.commons.dto.payload.ListKeysResponsePayload;
import se.digg.wallet.r2ps.commons.exception.ServiceRequestException;
import se.digg.wallet.r2ps.commons.exception.ServiceRequestHandlingException;
import se.digg.wallet.r2ps.infrastructure.config.KeyProviderBundle;
import se.digg.wallet.r2ps.server.service.servicehandlers.HsmServiceHandler;

import javax.crypto.KeyAgreement;
import javax.crypto.ShortBufferException;
import java.security.InvalidKeyException;
import java.security.KeyPair;
import java.security.NoSuchAlgorithmException;
import java.security.PublicKey;
import java.security.Signature;
import java.security.SignatureException;
import java.security.interfaces.ECKey;
import java.security.interfaces.ECPublicKey;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Optional;

/**
 * A service handler implementation for managing cryptographic operations with an HSM.
 * <p>
 * This class extends the functionality provided by the HsmServiceHandler superclass and is
 * customized for handling specific key management policies and cryptographic operations.
 *
 * TODO Include registry for key records. Figure out how to synchronize with KeyStores.
 */
@Slf4j
public class GenericHSMServiceHandler extends HsmServiceHandler {

  private final EcKeyPairRecordRegistry keyRegistry;
  private final Map<String, KeyProviderBundle> keyProviderBundles;

  /**
   * Constructs an instance of HsmServiceHandler with the provided supported curves and contexts.
   *
   * @param supportedCurves the list of elliptic curves supported by this HSM service handler
   * @param supportedContexts the list of operation contexts supported by this HSM service handler
   */
  public GenericHSMServiceHandler(final List<String> supportedCurves,
      final List<String> supportedContexts,
      final EcKeyPairRecordRegistry keyRegistry,
      final Map<String, KeyProviderBundle> keyProviderBundles) {
    super(supportedCurves, supportedContexts);
    this.keyRegistry = keyRegistry;
    this.keyProviderBundles = keyProviderBundles;
  }

  /**
   * Determines whether a request for a key operation is accepted based on the client's key pairs.
   * <p>
   * If the provided client ID does not have any associated key pairs, the method will accept the
   * request. Otherwise, it validates that none of the client's existing key pairs use the curve
   * specified by the requested curve name.
   * <p>
   * This is appropriate for test. Production variations of this class should also evaluate whether
   * the request suits a valid need for re-keying and should take into account the existing key
   * count and creation times.
   * <p>
   *
   * @param clientId the unique identifier representing the client making the request
   * @param keyRequestCurveName the name of the elliptic curve being requested for a key operation
   * @return {@code true} if the request is accepted (i.e., no conflicting key pair exists for the
   *         client and curve name), otherwise {@code false}
   */
  @Override
  protected void validateAgainstKeyGenerationPolicy(final String clientId,
      final String keyRequestCurveName)
      throws ServiceRequestHandlingException {
    if (keyRegistry.numberOfKeys(clientId, keyRequestCurveName) >= 2) {
      throw new ServiceRequestHandlingException(
          "The this curve already has the maximum number of keys (2)",
          ErrorCode.UNAUTHORIZED);
    }
  }

  @Override
  protected List<ListKeysResponsePayload.KeyInfo> getKeyInfo(final String clientId,
      List<String> requestedCurveNames)
      throws ServiceRequestException {

    List<ListKeysResponsePayload.KeyInfo> keyInfoList = new ArrayList<>();
    for (String curveName : requestedCurveNames) {
      if (!supportedCurves.contains(curveName)) {
        throw new IllegalArgumentException("Unsupported curve name: " + curveName);
      }

    }
    List<EcKeyPairRecord> clientRecords = keyRegistry.getClientRecords(clientId);
    for (EcKeyPairRecord record : clientRecords) {
      if (requestedCurveNames.isEmpty() || requestedCurveNames.contains(record.curveName())) {
        keyInfoList.add(new ListKeysResponsePayload.KeyInfo(record.kid(), record.curveName(),
            record.creationTime(),
            keyRegistry.getKey(clientId, record.kid()).getPublic()));
      }
    }
    return keyInfoList;
  }

  @Override
  protected void generateKey(final String clientId, final String keyRequestCurveName)
      throws ServiceRequestHandlingException {
    try {
      final KeyProviderBundle kpBundle = getKeyProviderBundle(keyRequestCurveName);
      keyRegistry.generateAndStoreKey(clientId, kpBundle);
    } catch (Exception e) {
      throw new ServiceRequestHandlingException("Failed to generate key: " + e.getMessage(),
          ErrorCode.SERVER_ERROR);
    }
  }

  @Override
  protected void deleteKey(final String clientId, final String kid)
      throws ServiceRequestHandlingException {
    try {
      keyRegistry.deleteKey(clientId, kid);
    } catch (ServiceRequestException e) {
      throw new ServiceRequestHandlingException("Unable to remove key key store",
          ErrorCode.SERVER_ERROR);
    }
  }

  @Override
  protected byte[] diffieHellman(final String clientId, final String kid, final PublicKey publicKey)
      throws ServiceRequestHandlingException {
    try {
      Optional<EcKeyPairRecord> recordOptional = keyRegistry.getRecord(clientId, kid);
      if (recordOptional.isEmpty()) {
        throw new ServiceRequestHandlingException("Key not found", ErrorCode.ACCESS_DENIED);
      }
      EcKeyPairRecord keyPairRecord = recordOptional.get();
      final KeyProviderBundle kpBundle = getKeyProviderBundle(keyPairRecord.curveName());
      KeyPair keyPair = keyRegistry.getKey(clientId, kid);

      // Perform DH key agreement
      KeyAgreement keyAgreement = KeyAgreement.getInstance("ECDH", kpBundle.getProvider());
      try {
        keyAgreement.init(keyPair.getPrivate());
        keyAgreement.doPhase(publicKey, true);
        // figure out Z length from the EC curve size
        int fieldSize = ((ECKey) publicKey).getParams().getCurve().getField().getFieldSize();
        int zLen = (fieldSize + 7) / 8; // P-256→32, P-384→48, P-521→66

        byte[] sharedSecret = new byte[zLen];
        keyAgreement.generateSecret(sharedSecret, 0);
        return sharedSecret;
      } catch (InvalidKeyException | ShortBufferException e) {
        throw new ServiceRequestHandlingException("Curve mismatch between public and private key",
            ErrorCode.ILLEGAL_REQUEST_DATA);
      }

    } catch (NoSuchAlgorithmException | ServiceRequestException e) {
      throw new ServiceRequestHandlingException("Error performing DH operation: " + e.getMessage(),
          ErrorCode.SERVER_ERROR);
    }
  }

  @Override
  protected byte[] ecdsaSignHashed(final String clientId, final String kid,
      final byte[] signRequestHashedData)
      throws ServiceRequestHandlingException {
    try {
      Optional<EcKeyPairRecord> recordOptional = keyRegistry.getRecord(clientId, kid);
      if (recordOptional.isEmpty()) {
        throw new ServiceRequestHandlingException("Key not found", ErrorCode.ACCESS_DENIED);
      }
      EcKeyPairRecord keyPairRecord = recordOptional.get();
      final KeyProviderBundle kpBundle = getKeyProviderBundle(keyPairRecord.curveName());
      KeyPair keyPair = keyRegistry.getKey(clientId, kid);
      ECPublicKey publicKey = (ECPublicKey) keyPair.getPublic();
      String algorithm = "NONEwithECDSA";

      // Check hashed data len is correct
      int fieldSize = (publicKey.getParams().getCurve().getField().getFieldSize() + 7) / 8;
      if (signRequestHashedData.length > fieldSize) {
        throw new ServiceRequestHandlingException(
            String.format("Hash length %d exceeds curve field size %d",
                signRequestHashedData.length, fieldSize),
            ErrorCode.ILLEGAL_REQUEST_DATA);
      }

      Signature signature = Signature.getInstance(algorithm, kpBundle.getProvider());
      signature.initSign(keyPair.getPrivate());
      signature.update(signRequestHashedData);
      return signature.sign();
    } catch (NoSuchAlgorithmException | InvalidKeyException | SignatureException
        | ServiceRequestException e) {
      throw new ServiceRequestHandlingException(
          "Error performing signature operation: " + e.getMessage(),
          ErrorCode.SERVER_ERROR);
    }
  }

  KeyProviderBundle getKeyProviderBundle(String curve) throws ServiceRequestException {
    if (!supportedCurves.contains(curve)) {
      throw new ServiceRequestException("Unsupported curve request");
    }
    if (keyProviderBundles.containsKey(curve)) {
      return keyProviderBundles.get(curve);
    }
    throw new ServiceRequestException("Unsupported curve request");
  }

}
