package se.digg.wallet.r2ps.domain.model;

import com.fasterxml.jackson.annotation.JsonInclude;
import java.util.UUID;

public record HsmWorkerRequest(
    UUID correlationId,
    String clientId,
    @JsonInclude(JsonInclude.Include.NON_NULL) String requestId,
    String stateJws,
    String outerRequestJws) {}
