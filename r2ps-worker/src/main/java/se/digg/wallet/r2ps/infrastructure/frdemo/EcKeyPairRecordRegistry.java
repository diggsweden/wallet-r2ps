package se.digg.wallet.r2ps.infrastructure.frdemo;

import se.digg.wallet.r2ps.commons.exception.ServiceRequestException;
import se.digg.wallet.r2ps.infrastructure.config.KeyProviderBundle;

import java.security.KeyPair;
import java.util.List;
import java.util.Optional;

/**
 * EcKeyPairRecordRegistry provides methods to manage elliptic curve key pair records. It allows
 * retrieval, storage, deletion, and synchronization of key pair records associated with specific
 * clients and key identifiers (KIDs). The registry can be used to manage key pairs in environments
 * where key management services are required.
 */
public interface EcKeyPairRecordRegistry {

  Optional<EcKeyPairRecord> getRecord(String clientId, final String kid)
      throws ServiceRequestException;

  List<EcKeyPairRecord> getClientRecords(String clientId);

  KeyPair getKey(final String clientId, final String kid) throws ServiceRequestException;

  void generateAndStoreKey(final String clientId, final KeyProviderBundle kpBundle)
      throws ServiceRequestException;

  void deleteKey(final String clientId, final String kid) throws ServiceRequestException;

  void synchronize() throws ServiceRequestException;

  long numberOfKeys(String clientId, String curve);

}
