package se.digg.wallet.r2ps.infrastructure.config;

import com.fasterxml.jackson.annotation.JsonInclude;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.dataformat.yaml.YAMLFactory;
import com.fasterxml.jackson.datatype.jsr310.JavaTimeModule;
import com.nimbusds.jose.JOSEException;
import com.nimbusds.jose.JWSAlgorithm;
import org.bouncycastle.util.encoders.Hex;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.context.annotation.Bean;
import org.springframework.context.annotation.Primary;
import org.springframework.data.redis.core.RedisTemplate;
import org.springframework.stereotype.Component;
import se.digg.wallet.r2ps.commons.dto.servicetype.ServiceTypeRegistry;
import se.digg.wallet.r2ps.commons.dto.servicetype.SessionTaskRegistry;
import se.digg.wallet.r2ps.commons.pake.opaque.InMemoryPakeSessionRegistry;
import se.digg.wallet.r2ps.commons.pake.opaque.OpaqueConfiguration;
import se.digg.wallet.r2ps.commons.pake.opaque.PakeSessionRegistry;
import se.digg.wallet.r2ps.application.port.out.AuthorizationCodeSpiPort;
import se.digg.wallet.r2ps.application.service.R2PSReplayChecker;
import se.digg.wallet.r2ps.infrastructure.AuthzRegistrationServiceHandler;
import se.digg.wallet.r2ps.infrastructure.adapter.out.AuthorizationCodeValKey;
import se.digg.wallet.r2ps.infrastructure.adapter.out.DeviceKeyRegistrySpiPortService;
import se.digg.wallet.r2ps.infrastructure.adapter.out.persistence.ServerWalletRegistry;
import se.digg.wallet.r2ps.infrastructure.frdemo.EcKeyPairRecordRegistry;
import se.digg.wallet.r2ps.infrastructure.frdemo.GenericHSMServiceHandler;
import se.digg.wallet.r2ps.server.pake.opaque.ClientRecordRegistry;
import se.digg.wallet.r2ps.server.pake.opaque.ServerPakeRecord;
import se.digg.wallet.r2ps.server.pake.opaque.impl.FileBackedClientRecordRegistry;
import se.digg.wallet.r2ps.server.service.ClientPublicKeyRecord;
import se.digg.wallet.r2ps.server.service.ClientPublicKeyRegistry;
import se.digg.wallet.r2ps.server.service.OpaqueServiceRequestHandlerConfiguration;
import se.digg.wallet.r2ps.server.service.ServiceRequestDispatcher;
import se.digg.wallet.r2ps.server.service.ServiceRequestHandler;
import se.digg.wallet.r2ps.server.service.impl.DefaultServiceRequestHandler;
import se.digg.wallet.r2ps.server.service.pinauthz.impl.CodeMatchPinAuthorization;
import se.digg.wallet.r2ps.server.service.servicehandlers.OpaqueServiceHandler;
import se.digg.wallet.r2ps.server.service.servicehandlers.ServiceTypeHandler;
import se.digg.wallet.r2ps.server.service.servicehandlers.SessionServiceHandler;
import se.swedenconnect.security.credential.PkiCredential;
import se.swedenconnect.security.credential.bundle.CredentialBundles;

import java.io.File;
import java.io.FileInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.security.KeyPair;
import java.security.cert.CertificateException;
import java.security.cert.CertificateFactory;
import java.time.Duration;
import java.util.Arrays;
import java.util.List;
import java.util.Map;

@Component
public class R2psBaseConfig {
  public static final ObjectMapper YAML_MAPPER;
  private static final Logger log = LoggerFactory.getLogger(R2psBaseConfig.class);

  static {
    YAML_MAPPER = new ObjectMapper(new YAMLFactory());
    YAML_MAPPER.setSerializationInclusion(JsonInclude.Include.NON_NULL);
    YAML_MAPPER.registerModule(new JavaTimeModule());
  }

  @Bean
  ServiceRequestHandler opaqueServiceRequestHandler(
      OpaqueServiceRequestHandlerConfiguration requestHandlerConfiguration)
      throws JOSEException {
    return new DefaultServiceRequestHandler(requestHandlerConfiguration);
  }

  @Bean
  public OpaqueServiceRequestHandlerConfiguration opaqueServiceRequestHandlerConfiguration(
      CredentialBundles credentialBundles, R2psBaseServerProperties rpsOpsServerProperties,
      ServiceTypeRegistry serviceTypeRegistry,
      List<ServiceRequestDispatcher> serviceRequestDispatchers,
      // List<ServiceTypeHandler> serviceTypeHandlerList,
      PakeSessionRegistry<ServerPakeRecord> serverPakeSessionRegistry,
      ClientPublicKeyRegistry clientPublicKeyRegistry,
      GenericHSMServiceHandler genericHSMServiceHandler,
      SessionServiceHandler sessionServiceHandler,
      SessionTaskRegistry sessionTaskRegistry, ClientRecordRegistry clientRecordRegistry) {

    log.info("SERVER OPAQUE KEY: {}", rpsOpsServerProperties.getServerOpaqueKey());
    final PkiCredential opaqueCredential =
        credentialBundles.getCredential(rpsOpsServerProperties.getServerOpaqueKey());
    final Map<String, Object> serverKeyProp =
        credentialBundles.getCredential(rpsOpsServerProperties.getServerOpaqueKey()).getMetadata()
            .getProperties();
    JWSAlgorithm serverJwsAlgorithm =
        JWSAlgorithm.parse((String) serverKeyProp.get("jws-algorithm"));

    // final PkiCredential opaqueCredential =
    // credentialBundles.getCredential("rhsm-server");


    OpaqueServiceHandler opaqueServiceHandler = new OpaqueServiceHandler(
        List.of("hsm"),
        new CodeMatchPinAuthorization(clientPublicKeyRegistry),
        OpaqueConfiguration.defaultConfiguration(),
        "https://cloud-wallet.digg.se/rhsm",
        Hex.decode("9aba66b536549dc6630f719bbcbaa16cbf70253d273640d7690f6e2e4ef69875"),
        new KeyPair(opaqueCredential.getPublicKey(), opaqueCredential.getPrivateKey()),
        serverPakeSessionRegistry,
        clientRecordRegistry,
        sessionTaskRegistry,
        Duration.ofMinutes(15),
        Duration.ofSeconds(5));

    List<ServiceTypeHandler> serviceTypeHandlerList = List.of(
        opaqueServiceHandler,
        sessionServiceHandler,

        genericHSMServiceHandler,
        new AuthzRegistrationServiceHandler(clientPublicKeyRegistry));

    return OpaqueServiceRequestHandlerConfiguration.builder()
        .serverKeyPair(
            new KeyPair(opaqueCredential.getPublicKey(), opaqueCredential.getPrivateKey()))
        .serverJwsAlgorithm(serverJwsAlgorithm)
        .serverPakeSessionRegistry(serverPakeSessionRegistry)
        .clientPublicKeyRegistry(clientPublicKeyRegistry)
        .serviceTypeRegistry(serviceTypeRegistry)
        .serviceTypeHandlers(serviceTypeHandlerList)
        .replayChecker(new R2PSReplayChecker(Duration.ofHours(24)))
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
  ClientPublicKeyRegistry clientPublicKeyRegistry(R2psBaseServerProperties rpsOpsServerProperties,
      ServerWalletRegistry serverWalletRegistry, AuthorizationCodeSpiPort authorizationCodeSpiPort)
      throws IOException {


    ClientPublicKeyRegistry clientPublicKeyRegistry =
        new DeviceKeyRegistrySpiPortService(authorizationCodeSpiPort, serverWalletRegistry);

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
        try {
          clientPublicKeyRegistry.registerClientPublicKey(client.getClientId(),
              ClientPublicKeyRecord.builder()
                  .publicKey(cf.generateCertificate(is).getPublicKey())
                  .supportedContexts(client.getContexts())
                  .kid(client.getKid()).build());
        } catch (RuntimeException e) {
          throw new RuntimeException(e);
        }
      } catch (CertificateException e) {
        throw new RuntimeException(e);
      }
    }
    return clientPublicKeyRegistry;
  }

  /*
   * @Bean List<ServiceTypeHandler> serviceTypeHandlerList(SessionServiceHandler
   * sessionServiceHandler, ClientPublicKeyRegistry clientPublicKeyRegistry, CredentialBundles
   * credentialBundles, PakeSessionRegistry<ServerPakeRecord> serverPakeSessionRegistry,
   * SessionTaskRegistry sessionTaskRegistry, ClientRecordRegistry clientRecordRegistry) {
   *
   * final PkiCredential opaqueCredential = credentialBundles.getCredential("rhsm-server");
   *
   *
   * OpaqueServiceHandler opaqueServiceHandler = new OpaqueServiceHandler( List.of("hsm"), new
   * CodeMatchPinAuthorization(clientPublicKeyRegistry), OpaqueConfiguration.defaultConfiguration(),
   * "https://cloud-wallet.digg.se/rhsm",
   * Hex.decode("9aba66b536549dc6630f719bbcbaa16cbf70253d273640d7690f6e2e4ef69875"), new
   * KeyPair(opaqueCredential.getPublicKey(), opaqueCredential.getPrivateKey()),
   * serverPakeSessionRegistry, clientRecordRegistry, sessionTaskRegistry, Duration.ofMinutes(15),
   * Duration.ofSeconds(5));
   *
   * return List.of( opaqueServiceHandler, sessionServiceHandler, new
   * AuthzRegistrationServiceHandler(clientPublicKeyRegistry)); }
   */
  @Bean
  SessionServiceHandler sessionServiceHandler(
      PakeSessionRegistry<ServerPakeRecord> pakeSessionRegistry) {
    return new SessionServiceHandler(pakeSessionRegistry);
  }

  @Bean
  ClientRecordRegistry clientRecordRegistry() throws IOException {
    return new FileBackedClientRecordRegistry(
        ConfigUtils.getFile("foo/client-record-registry.json", true));
  }

  @Bean
  AuthorizationCodeSpiPort authorizationCodeSpiPort(RedisTemplate<String, String> redisTemplate) {
    return new AuthorizationCodeValKey(redisTemplate);
  }

  @Bean
  SessionTaskRegistry sessionTaskRegistry() {
    SessionTaskRegistry sessionTaskRegistry = new SessionTaskRegistry();
    Arrays.stream(SessionTaskId.values()).forEach(sessionTaskId -> {
      sessionTaskRegistry.registerSessionTask(sessionTaskId.name(),
          sessionTaskId.getSessionDuration());
    });
    return sessionTaskRegistry;
  }

  public enum SessionTaskId {

    general(Duration.ofMinutes(15)),
    sign(Duration.ofSeconds(30)),
    hsm(Duration.ofMinutes(1));

    private Duration sessionDuration;

    SessionTaskId(Duration duration) {}

    public Duration getSessionDuration() {
      return sessionDuration;
    }
  }

}
