package se.digg.wallet.r2ps.commons.dto;

import com.fasterxml.jackson.annotation.JsonProperty;
import java.time.Instant;

/** Abstract class for signed service exchange messages (Requests and Responses). */
public abstract class ServiceExchange {

  /** Indicates the version of this service exchange protocol */
  @JsonProperty("version")
  private Integer version;

  /** Payload nonce that is sent by the client and returned by the server */
  @JsonProperty("nonce")
  private String nonce;

  /** The time when this service exchange object was created */
  @JsonProperty("iat")
  private Instant iat;

  /** Indicates if the serviceData is encrypted or holds plaintext data */
  // @JsonProperty("enc")
  // private EncryptOption encryptOption;

  /** JSON data or a JWE with encrypted JSON payload */
  @JsonProperty("inner_jwe")
  private String innerJwe;

  public ServiceExchange() {
    this.version = 1;
  }

  public Integer getVersion() {
    return version;
  }

  public void setVersion(final Integer version) {
    this.version = version;
  }

  public String getNonce() {
    return nonce;
  }

  public void setNonce(final String nonce) {
    this.nonce = nonce;
  }

  public Instant getIat() {
    return iat;
  }

  public void setIat(final Instant iat) {
    this.iat = iat;
  }

  // public EncryptOption getEncryptOption() {
  //   return encryptOption;
  // }

  // public void setEncryptOption(final EncryptOption encryptOption) {
  //   this.encryptOption = encryptOption;
  // }

  @JsonProperty("session_id")
  private String sessionId;

  public String getSessionId() {
    return sessionId;
  }

  public void setSessionId(final String sessionId) {
    this.sessionId = sessionId;
  }

  public String getInnerJwe() {
    return innerJwe;
  }

  public void setInnerJwe(final String innerJwe) {
    this.innerJwe = innerJwe;
  }

  public abstract static class AbstractBuilder<T extends ServiceExchange, B extends AbstractBuilder<?, ?>> {
    protected T serviceExchange;

    public AbstractBuilder(final T serviceExchange) {
      this.serviceExchange = serviceExchange;
    }

    protected abstract B getBuilder();

    protected abstract void validate();

    public B nonce(final String nonce) {
      this.serviceExchange.setNonce(nonce);
      return getBuilder();
    }

    public T build() {
      this.serviceExchange.setIat(Instant.now());
      validate();
      return this.serviceExchange;
    }
  }
}
