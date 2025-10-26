package se.digg.wallet.r2ps.infrastructure.adapter.out.serverwallet.entity;

import io.hypersistence.utils.hibernate.type.json.JsonBinaryType;
import jakarta.persistence.*;
import org.hibernate.annotations.JdbcTypeCode;
import org.hibernate.annotations.Type;
import org.hibernate.type.SqlTypes;
import se.digg.wallet.r2ps.domain.model.aggregate.ServerWallet;

import java.time.LocalDateTime;
import java.util.UUID;

@Entity
@Table(name = "server_wallets")
public class ServerWalletEntity {

  @Id
  private UUID walletId;


  @Type(JsonBinaryType.class)
  @JdbcTypeCode(SqlTypes.JSON)
  @Column(columnDefinition = "jsonb")
  private ServerWallet data;


  @Column(nullable = false)
  private LocalDateTime timestamp;

  @PrePersist
  protected void onCreate() {
    if (timestamp == null) {
      timestamp = LocalDateTime.now();
    }
  }

  // Constructors

  public ServerWalletEntity() {
  }

  public ServerWalletEntity(UUID walletId, ServerWallet data) {
    this.walletId = walletId;
    this.data = data;
  }

  // Getters and Setters
  public UUID getWalletId() {
    return walletId;
  }

  public void setWalletId(UUID id) {
    this.walletId = id;
  }

  public ServerWallet getData() {
    return data;
  }

  public void setData(ServerWallet data) {
    this.data = data;
  }

  public LocalDateTime getTimestamp() {
    return timestamp;
  }

  public void setTimestamp(LocalDateTime timestamp) {
    this.timestamp = timestamp;
  }
}
