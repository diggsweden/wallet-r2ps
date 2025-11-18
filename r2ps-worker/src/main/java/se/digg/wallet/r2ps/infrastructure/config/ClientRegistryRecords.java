package se.digg.wallet.r2ps.infrastructure.config;

import com.fasterxml.jackson.annotation.JsonProperty;

import java.util.List;

public class ClientRegistryRecords {

  private List<ClientRegistryRecord> clients;

  public ClientRegistryRecords(List<ClientRegistryRecord> clients) {
    this.clients = clients;
  }

  public ClientRegistryRecords() {}

  public List<ClientRegistryRecord> getClients() {
    return this.clients;
  }

  public void setClients(List<ClientRegistryRecord> clients) {
    this.clients = clients;
  }

  public boolean equals(final Object o) {
    if (o == this)
      return true;
    if (!(o instanceof ClientRegistryRecords))
      return false;
    final ClientRegistryRecords other = (ClientRegistryRecords) o;
    if (!other.canEqual((Object) this))
      return false;
    final Object this$clients = this.getClients();
    final Object other$clients = other.getClients();
    if (this$clients == null ? other$clients != null : !this$clients.equals(other$clients))
      return false;
    return true;
  }

  protected boolean canEqual(final Object other) {
    return other instanceof ClientRegistryRecords;
  }

  public int hashCode() {
    final int PRIME = 59;
    int result = 1;
    final Object $clients = this.getClients();
    result = result * PRIME + ($clients == null ? 43 : $clients.hashCode());
    return result;
  }

  public String toString() {
    return "ClientRegistryRecords(clients=" + this.getClients() + ")";
  }


  public static class ClientRegistryRecord {

    @JsonProperty("client-cert")
    String clientCert;
    @JsonProperty("client-id")
    String clientId;
    @JsonProperty("kid")
    String kid;
    @JsonProperty("contexts")
    List<String> contexts;

    public ClientRegistryRecord(String clientCert, String clientId, String kid,
        List<String> contexts) {
      this.clientCert = clientCert;
      this.clientId = clientId;
      this.kid = kid;
      this.contexts = contexts;
    }

    public ClientRegistryRecord() {}

    public String getClientCert() {
      return this.clientCert;
    }

    public String getClientId() {
      return this.clientId;
    }

    public String getKid() {
      return this.kid;
    }

    public List<String> getContexts() {
      return this.contexts;
    }

    @JsonProperty("client-cert")
    public void setClientCert(String clientCert) {
      this.clientCert = clientCert;
    }

    @JsonProperty("client-id")
    public void setClientId(String clientId) {
      this.clientId = clientId;
    }

    @JsonProperty("kid")
    public void setKid(String kid) {
      this.kid = kid;
    }

    @JsonProperty("contexts")
    public void setContexts(List<String> contexts) {
      this.contexts = contexts;
    }

    public boolean equals(final Object o) {
      if (o == this)
        return true;
      if (!(o instanceof ClientRegistryRecord))
        return false;
      final ClientRegistryRecord other = (ClientRegistryRecord) o;
      if (!other.canEqual((Object) this))
        return false;
      final Object this$clientCert = this.getClientCert();
      final Object other$clientCert = other.getClientCert();
      if (this$clientCert == null ? other$clientCert != null
          : !this$clientCert.equals(other$clientCert))
        return false;
      final Object this$clientId = this.getClientId();
      final Object other$clientId = other.getClientId();
      if (this$clientId == null ? other$clientId != null : !this$clientId.equals(other$clientId))
        return false;
      final Object this$kid = this.getKid();
      final Object other$kid = other.getKid();
      if (this$kid == null ? other$kid != null : !this$kid.equals(other$kid))
        return false;
      final Object this$contexts = this.getContexts();
      final Object other$contexts = other.getContexts();
      if (this$contexts == null ? other$contexts != null : !this$contexts.equals(other$contexts))
        return false;
      return true;
    }

    protected boolean canEqual(final Object other) {
      return other instanceof ClientRegistryRecord;
    }

    public int hashCode() {
      final int PRIME = 59;
      int result = 1;
      final Object $clientCert = this.getClientCert();
      result = result * PRIME + ($clientCert == null ? 43 : $clientCert.hashCode());
      final Object $clientId = this.getClientId();
      result = result * PRIME + ($clientId == null ? 43 : $clientId.hashCode());
      final Object $kid = this.getKid();
      result = result * PRIME + ($kid == null ? 43 : $kid.hashCode());
      final Object $contexts = this.getContexts();
      result = result * PRIME + ($contexts == null ? 43 : $contexts.hashCode());
      return result;
    }

    public String toString() {
      return "ClientRegistryRecords.ClientRegistryRecord(clientCert=" + this.getClientCert()
          + ", clientId=" + this.getClientId() + ", kid=" + this.getKid() + ", contexts="
          + this.getContexts() + ")";
    }
  }

}
