package se.digg.wallet.r2ps.infrastructure.config;

import org.springframework.boot.context.properties.ConfigurationProperties;
import org.springframework.context.annotation.Configuration;

import java.time.Duration;
import java.util.Map;

@Configuration
@ConfigurationProperties(prefix = "rps-ops")
public class R2psBaseServerProperties {

  private String serverIdentity;
  private String oprfSeed;
  private String serverOpaqueKey;
  private String serverHsmKey;
  private String clientRecordRegistryFile;
  private String clientRegistryInitDirectory;
  private Duration sessionDuration;
  private Duration finalizeDuration;
  private Duration replayCheckDuration;
  private Map<String, String> contextUrl;

  public R2psBaseServerProperties() {
  }

  public String getServerIdentity() {
    return this.serverIdentity;
  }

  public String getOprfSeed() {
    return this.oprfSeed;
  }

  public String getServerOpaqueKey() {
    return this.serverOpaqueKey;
  }

  public String getServerHsmKey() {
    return this.serverHsmKey;
  }

  public String getClientRecordRegistryFile() {
    return this.clientRecordRegistryFile;
  }

  public String getClientRegistryInitDirectory() {
    return this.clientRegistryInitDirectory;
  }

  public Duration getSessionDuration() {
    return this.sessionDuration;
  }

  public Duration getFinalizeDuration() {
    return this.finalizeDuration;
  }

  public Duration getReplayCheckDuration() {
    return this.replayCheckDuration;
  }

  public Map<String, String> getContextUrl() {
    return this.contextUrl;
  }

  public void setServerIdentity(String serverIdentity) {
    this.serverIdentity = serverIdentity;
  }

  public void setOprfSeed(String oprfSeed) {
    this.oprfSeed = oprfSeed;
  }

  public void setServerOpaqueKey(String serverOpaqueKey) {
    this.serverOpaqueKey = serverOpaqueKey;
  }

  public void setServerHsmKey(String serverHsmKey) {
    this.serverHsmKey = serverHsmKey;
  }

  public void setClientRecordRegistryFile(String clientRecordRegistryFile) {
    this.clientRecordRegistryFile = clientRecordRegistryFile;
  }

  public void setClientRegistryInitDirectory(String clientRegistryInitDirectory) {
    this.clientRegistryInitDirectory = clientRegistryInitDirectory;
  }

  public void setSessionDuration(Duration sessionDuration) {
    this.sessionDuration = sessionDuration;
  }

  public void setFinalizeDuration(Duration finalizeDuration) {
    this.finalizeDuration = finalizeDuration;
  }

  public void setReplayCheckDuration(Duration replayCheckDuration) {
    this.replayCheckDuration = replayCheckDuration;
  }

  public void setContextUrl(Map<String, String> contextUrl) {
    this.contextUrl = contextUrl;
  }

  public boolean equals(final Object o) {
    if (o == this)
      return true;
    if (!(o instanceof R2psBaseServerProperties))
      return false;
    final R2psBaseServerProperties other = (R2psBaseServerProperties) o;
    if (!other.canEqual((Object) this))
      return false;
    final Object this$serverIdentity = this.getServerIdentity();
    final Object other$serverIdentity = other.getServerIdentity();
    if (this$serverIdentity == null ?
        other$serverIdentity != null :
        !this$serverIdentity.equals(other$serverIdentity))
      return false;
    final Object this$oprfSeed = this.getOprfSeed();
    final Object other$oprfSeed = other.getOprfSeed();
    if (this$oprfSeed == null ? other$oprfSeed != null : !this$oprfSeed.equals(other$oprfSeed))
      return false;
    final Object this$serverOpaqueKey = this.getServerOpaqueKey();
    final Object other$serverOpaqueKey = other.getServerOpaqueKey();
    if (this$serverOpaqueKey == null ?
        other$serverOpaqueKey != null :
        !this$serverOpaqueKey.equals(other$serverOpaqueKey))
      return false;
    final Object this$serverHsmKey = this.getServerHsmKey();
    final Object other$serverHsmKey = other.getServerHsmKey();
    if (this$serverHsmKey == null ?
        other$serverHsmKey != null :
        !this$serverHsmKey.equals(other$serverHsmKey))
      return false;
    final Object this$clientRecordRegistryFile = this.getClientRecordRegistryFile();
    final Object other$clientRecordRegistryFile = other.getClientRecordRegistryFile();
    if (this$clientRecordRegistryFile == null ?
        other$clientRecordRegistryFile != null :
        !this$clientRecordRegistryFile.equals(other$clientRecordRegistryFile))
      return false;
    final Object this$clientRegistryInitDirectory = this.getClientRegistryInitDirectory();
    final Object other$clientRegistryInitDirectory = other.getClientRegistryInitDirectory();
    if (this$clientRegistryInitDirectory == null ?
        other$clientRegistryInitDirectory != null :
        !this$clientRegistryInitDirectory.equals(other$clientRegistryInitDirectory))
      return false;
    final Object this$sessionDuration = this.getSessionDuration();
    final Object other$sessionDuration = other.getSessionDuration();
    if (this$sessionDuration == null ?
        other$sessionDuration != null :
        !this$sessionDuration.equals(other$sessionDuration))
      return false;
    final Object this$finalizeDuration = this.getFinalizeDuration();
    final Object other$finalizeDuration = other.getFinalizeDuration();
    if (this$finalizeDuration == null ?
        other$finalizeDuration != null :
        !this$finalizeDuration.equals(other$finalizeDuration))
      return false;
    final Object this$replayCheckDuration = this.getReplayCheckDuration();
    final Object other$replayCheckDuration = other.getReplayCheckDuration();
    if (this$replayCheckDuration == null ?
        other$replayCheckDuration != null :
        !this$replayCheckDuration.equals(other$replayCheckDuration))
      return false;
    final Object this$contextUrl = this.getContextUrl();
    final Object other$contextUrl = other.getContextUrl();
    if (this$contextUrl == null ?
        other$contextUrl != null :
        !this$contextUrl.equals(other$contextUrl))
      return false;
    return true;
  }

  protected boolean canEqual(final Object other) {
    return other instanceof R2psBaseServerProperties;
  }

  public int hashCode() {
    final int PRIME = 59;
    int result = 1;
    final Object $serverIdentity = this.getServerIdentity();
    result = result * PRIME + ($serverIdentity == null ? 43 : $serverIdentity.hashCode());
    final Object $oprfSeed = this.getOprfSeed();
    result = result * PRIME + ($oprfSeed == null ? 43 : $oprfSeed.hashCode());
    final Object $serverOpaqueKey = this.getServerOpaqueKey();
    result = result * PRIME + ($serverOpaqueKey == null ? 43 : $serverOpaqueKey.hashCode());
    final Object $serverHsmKey = this.getServerHsmKey();
    result = result * PRIME + ($serverHsmKey == null ? 43 : $serverHsmKey.hashCode());
    final Object $clientRecordRegistryFile = this.getClientRecordRegistryFile();
    result = result * PRIME + ($clientRecordRegistryFile == null ?
        43 :
        $clientRecordRegistryFile.hashCode());
    final Object $clientRegistryInitDirectory = this.getClientRegistryInitDirectory();
    result = result * PRIME + ($clientRegistryInitDirectory == null ?
        43 :
        $clientRegistryInitDirectory.hashCode());
    final Object $sessionDuration = this.getSessionDuration();
    result = result * PRIME + ($sessionDuration == null ? 43 : $sessionDuration.hashCode());
    final Object $finalizeDuration = this.getFinalizeDuration();
    result = result * PRIME + ($finalizeDuration == null ? 43 : $finalizeDuration.hashCode());
    final Object $replayCheckDuration = this.getReplayCheckDuration();
    result = result * PRIME + ($replayCheckDuration == null ? 43 : $replayCheckDuration.hashCode());
    final Object $contextUrl = this.getContextUrl();
    result = result * PRIME + ($contextUrl == null ? 43 : $contextUrl.hashCode());
    return result;
  }

  public String toString() {
    return "R2psBaseServerProperties(serverIdentity=" + this.getServerIdentity() + ", oprfSeed=" + this.getOprfSeed() + ", serverOpaqueKey=" + this.getServerOpaqueKey() + ", serverHsmKey=" + this.getServerHsmKey() + ", clientRecordRegistryFile=" + this.getClientRecordRegistryFile() + ", clientRegistryInitDirectory=" + this.getClientRegistryInitDirectory() + ", sessionDuration=" + this.getSessionDuration() + ", finalizeDuration=" + this.getFinalizeDuration() + ", replayCheckDuration=" + this.getReplayCheckDuration() + ", contextUrl=" + this.getContextUrl() + ")";
  }
}
