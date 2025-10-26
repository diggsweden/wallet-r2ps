package se.digg.wallet.r2ps.infrastructure.config;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.dataformat.yaml.YAMLFactory;
import com.fasterxml.jackson.datatype.jsr310.JavaTimeModule;
import com.nimbusds.jose.JOSEException;
import com.nimbusds.jose.JWSAlgorithm;
import org.bouncycastle.util.encoders.Hex;
import org.springframework.context.annotation.Bean;
import org.springframework.stereotype.Component;
import se.digg.wallet.r2ps.commons.dto.servicetype.ServiceTypeRegistry;
import se.digg.wallet.r2ps.commons.pake.opaque.InMemoryPakeSessionRegistry;
import se.digg.wallet.r2ps.commons.pake.opaque.OpaqueConfiguration;
import se.digg.wallet.r2ps.commons.pake.opaque.PakeSessionRegistry;
import se.digg.wallet.r2ps.server.pake.opaque.ServerPakeRecord;
import se.digg.wallet.r2ps.server.pake.opaque.impl.FileBackedClientRecordRegistry;
import se.digg.wallet.r2ps.server.service.ClientPublicKeyRecord;
import se.digg.wallet.r2ps.server.service.ClientPublicKeyRegistry;
import se.digg.wallet.r2ps.server.service.OpaqueServiceRequestHandlerConfiguration;
import se.digg.wallet.r2ps.server.service.ServiceRequestDispatcher;
import se.digg.wallet.r2ps.server.service.ServiceRequestHandler;
import se.digg.wallet.r2ps.server.service.impl.FileBackedClientPublicKeyRegistry;
import se.digg.wallet.r2ps.server.service.impl.OpaqueServiceRequestHandler;
import se.digg.wallet.r2ps.server.service.impl.RpsOpsReplayChecker;
import se.digg.wallet.r2ps.server.service.servicehandlers.ServiceTypeHandler;
import se.swedenconnect.security.credential.PkiCredential;
import se.swedenconnect.security.credential.bundle.CredentialBundles;

import java.io.File;
import java.io.FileInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.security.KeyPair;
import java.security.cert.CertificateException;
import java.security.cert.CertificateFactory;
import java.util.List;
import java.util.Map;

@Component
public class R2psBaseConfig {
  public static final ObjectMapper YAML_MAPPER;

  static {
    YAML_MAPPER = new ObjectMapper(new YAMLFactory());
    YAML_MAPPER.setSerializationInclusion(JsonInclude.Include.NON_NULL);
    YAML_MAPPER.registerModule(new JavaTimeModule());
  }

  @Bean
  ServiceRequestHandler opaqueServiceRequestHandler(
      OpaqueServiceRequestHandlerConfiguration requestHandlerConfiguration)
      throws JOSEException {
    return new OpaqueServiceRequestHandler(requestHandlerConfiguration);
  }

  @Bean
  public OpaqueServiceRequestHandlerConfiguration opaqueServiceRequestHandlerConfiguration(
      CredentialBundles credentialBundles, R2psBaseServerProperties rpsOpsServerProperties,
      ServiceTypeRegistry serviceTypeRegistry,
      List<ServiceRequestDispatcher> serviceRequestDispatchers,
      List<ServiceTypeHandler> serviceTypeHandlerList,
      PakeSessionRegistry<ServerPakeRecord> serverPakeSessionRegistry,
      ClientPublicKeyRegistry clientPublicKeyRegistry) {

    final PkiCredential opaqueCredential =
        credentialBundles.getCredential(rpsOpsServerProperties.getServerOpaqueKey());
    final Map<String, Object> serverKeyProp =
        credentialBundles.getCredential(rpsOpsServerProperties.getServerOpaqueKey()).getMetadata()
            .getProperties();
    JWSAlgorithm serverJwsAlgorithm =
        JWSAlgorithm.parse((String) serverKeyProp.get("jws-algorithm"));

    return OpaqueServiceRequestHandlerConfiguration.builder()
        .serverIdentity(rpsOpsServerProperties.getServerIdentity())
        .opaqueConfiguration(OpaqueConfiguration.defaultConfiguration())
        .oprfSeed(Hex.decode(rpsOpsServerProperties.getOprfSeed()))
        .serverOpaqueKeyPair(
            new KeyPair(opaqueCredential.getPublicKey(), opaqueCredential.getPrivateKey()))
        .serverJwsAlgorithm(serverJwsAlgorithm)
        .serverPakeSessionRegistry(serverPakeSessionRegistry)
        .clientPublicKeyRegistry(clientPublicKeyRegistry)
        .clientRecordRegistry(new FileBackedClientRecordRegistry(
            ConfigUtils.getFile(rpsOpsServerProperties.getClientRecordRegistryFile(), true)))
        .serviceTypeRegistry(serviceTypeRegistry)
        .serviceTypeHandlers(serviceTypeHandlerList)
        .serviceRequestDispatchers(serviceRequestDispatchers)
        .replayChecker(new RpsOpsReplayChecker(rpsOpsServerProperties.getReplayCheckDuration()))
        .sessionDuration(rpsOpsServerProperties.getSessionDuration())
        .fianlizeDuration(rpsOpsServerProperties.getFinalizeDuration())
        .build();
  }

  @Bean
  ServiceTypeRegistry serviceTypeRegistry() {
    return ConfigUtils.getDemoServiceTypeRegistry();
  }

  @Bean
  PakeSessionRegistry<ServerPakeRecord> serverPakeSessionRegistry() {
    return new InMemoryPakeSessionRegistry<>();
  }

  @Bean
  ClientPublicKeyRegistry clientPublicKeyRegistry(R2psBaseServerProperties rpsOpsServerProperties)
      throws IOException {

    ClientPublicKeyRegistry clientPublicKeyRegistry = new FileBackedClientPublicKeyRegistry(null);

    final File clientRegistryDir =
        ConfigUtils.getFile(rpsOpsServerProperties.getClientRegistryInitDirectory());
    final File clientRegistryFile = new File(clientRegistryDir, "clients.yml");
    final ClientRegistryRecords clientRegistryRecords =
        YAML_MAPPER.readValue(clientRegistryFile, ClientRegistryRecords.class);
    final List<ClientRegistryRecords.ClientRegistryRecord> clients =
        clientRegistryRecords.getClients();
    for (ClientRegistryRecords.ClientRegistryRecord client : clients) {
      final File certFile = new File(new File(clientRegistryDir, "certs"), client.getClientCert());
      try (InputStream is = new FileInputStream(certFile)) {
        CertificateFactory cf = CertificateFactory.getInstance("X.509");
        clientPublicKeyRegistry.registerClientPublicKey(client.getClientId(),
            ClientPublicKeyRecord.builder()
                .publicKey(cf.generateCertificate(is).getPublicKey())
                .supportedContexts(client.getContexts())
                .kid(client.getKid())
                .build());
      } catch (CertificateException e) {
        throw new RuntimeException(e);
      }
    }
    return clientPublicKeyRegistry;
  }
}
