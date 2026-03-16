use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Envelope for all client-to-server WebSocket messages.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsInbound {
    /// Step 1 of HPKE mutual auth: client announces its identity.
    AuthInit(AuthInitMsg),

    /// Step 3 of HPKE mutual auth: client responds to server challenge.
    AuthResponse(AuthResponseMsg),

    /// Submit a service request (post-auth, equivalent to POST /).
    Request(WsRequestMsg),
}

/// Auth init message — client announces its client_id and optionally
/// the `kid` of the key it will use for HPKE authentication.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthInitMsg {
    pub client_id: String,
    /// Key identifier for the client key to use. If `None`, the first
    /// available key for this client_id is used.
    #[serde(default)]
    pub kid: Option<String>,
}

/// Auth response message — client proves key possession via HPKE.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthResponseMsg {
    /// HPKE encapsulated key (base64url-encoded)
    pub enc: String,
    /// HPKE ciphertext (base64url-encoded)
    pub ciphertext: String,
}

/// A service request submitted over WebSocket.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WsRequestMsg {
    /// Client-generated request ID for response matching.
    pub request_id: String,
    /// JWS-signed service request envelope.
    pub outer_request_jws: String,
}

/// Envelope for all server-to-client WebSocket messages.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsOutbound {
    /// Step 2 of HPKE mutual auth: server challenge.
    AuthChallenge(AuthChallengeMsg),

    /// Step 4 of HPKE mutual auth: authentication succeeded.
    AuthOk(AuthOkMsg),

    /// Authentication failed.
    AuthError(WsErrorMsg),

    /// Response to a service request (pushed from Kafka).
    Response(WsResponseMsg),

    /// Error for a specific request.
    RequestError(WsRequestErrorMsg),

    /// Protocol-level error.
    Error(WsErrorMsg),
}

/// Server HPKE auth challenge.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthChallengeMsg {
    /// HPKE encapsulated key (base64url-encoded)
    pub enc: String,
    /// HPKE ciphertext containing the challenge nonce (base64url-encoded)
    pub ciphertext: String,
    /// Random salt (base64url-encoded, sent in the clear).
    /// The client must combine this with the decrypted nonce to produce
    /// HMAC-SHA256(key=nonce, msg=salt) as its auth response.
    pub salt: String,
    /// Server's public key identifier
    pub server_kid: String,
}

/// Auth success acknowledgment.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthOkMsg {
    pub client_id: String,
}

/// Response message pushed to client.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WsResponseMsg {
    /// Echoed from the client's request (if this was from a WS request).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Server-generated correlation ID.
    pub correlation_id: Uuid,
    /// Response status.
    pub status: String,
    /// Response JWS (present when status is "COMPLETE").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// Error info (present when status is "ERROR").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<WsResponseError>,
}

/// Error details within a response.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WsResponseError {
    pub message: String,
    pub http_status: u16,
}

/// Error for a specific request.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WsRequestErrorMsg {
    pub request_id: String,
    pub message: String,
}

/// Generic protocol error.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WsErrorMsg {
    pub message: String,
}
