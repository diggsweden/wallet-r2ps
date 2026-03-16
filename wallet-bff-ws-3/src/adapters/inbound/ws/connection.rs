//! Per-connection WebSocket state machine.
//!
//! States: AwaitingAuthInit -> AwaitingAuthResponse -> Authenticated
//!
//! Once authenticated, the connection:
//! - Accepts service requests from the client (same payload as POST /)
//! - Pushes all responses for the client_id from the broadcast channel

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use p256::PublicKey;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use crate::domain::{
    device_management::value_objects::ClientId, request_processing::value_objects::ProcessingMode,
};
use crate::ports::outbound::ClientKeyRepository;

use super::auth::{b64_decode, b64_encode, ec_public_key_from_device_state};
use super::dto::*;
use super::state::SharedWsState;

/// Run a single WebSocket connection to completion.
///
/// This is spawned as a task per connection from the upgrade handler.
pub async fn handle_connection(ws: WebSocket, state: SharedWsState) {
    let (mut ws_tx, mut ws_rx) = ws.split();

    // -- Phase 1: Authentication --

    // Wait for auth_init
    let (client_id_str, client_pk) = match await_auth_init(&mut ws_rx, &mut ws_tx, &state).await {
        Ok(result) => result,
        Err(msg) => {
            let _ = send_json(
                &mut ws_tx,
                &WsOutbound::AuthError(WsErrorMsg { message: msg }),
            )
            .await;
            return;
        }
    };

    // Create challenge
    let challenge = match state.hpke_auth.create_challenge(&client_pk) {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "HPKE challenge creation failed");
            let _ = send_json(
                &mut ws_tx,
                &WsOutbound::AuthError(WsErrorMsg {
                    message: "internal auth error".to_string(),
                }),
            )
            .await;
            return;
        }
    };

    // Send challenge (includes salt in the clear)
    let challenge_msg = WsOutbound::AuthChallenge(AuthChallengeMsg {
        enc: b64_encode(&challenge.enc),
        ciphertext: b64_encode(&challenge.ciphertext),
        salt: b64_encode(&challenge.salt),
        server_kid: state.hpke_auth.server_kid().to_string(),
    });
    if send_json(&mut ws_tx, &challenge_msg).await.is_err() {
        return;
    }

    // Wait for auth_response and verify (nonce + salt)
    match await_auth_response(
        &mut ws_rx,
        &state,
        &client_pk,
        &challenge.nonce,
        &challenge.salt,
    )
    .await
    {
        Ok(()) => {}
        Err(msg) => {
            let _ = send_json(
                &mut ws_tx,
                &WsOutbound::AuthError(WsErrorMsg { message: msg }),
            )
            .await;
            return;
        }
    }

    // Auth succeeded
    let _ = send_json(
        &mut ws_tx,
        &WsOutbound::AuthOk(AuthOkMsg {
            client_id: client_id_str.clone(),
        }),
    )
    .await;

    info!(client_id = %client_id_str, "WebSocket client authenticated");

    // -- Phase 2: Authenticated session --

    // Register in the client connection registry
    let mut broadcast_rx = state.registry.register(&client_id_str);

    // Split into concurrent tasks: one for inbound messages, one for broadcast
    let client_id_for_outbound = client_id_str.clone();
    let state_for_inbound = state.clone();

    // Task: forward broadcast messages to WebSocket
    let mut outbound_handle = tokio::spawn(async move {
        loop {
            match broadcast_rx.recv().await {
                Ok(response_msg) => {
                    let outbound = WsOutbound::Response(response_msg);
                    let json = match serde_json::to_string(&outbound) {
                        Ok(j) => j,
                        Err(e) => {
                            error!(error = %e, "failed to serialize WS response");
                            continue;
                        }
                    };
                    if ws_tx.send(Message::Text(json)).await.is_err() {
                        debug!(
                            client_id = %client_id_for_outbound,
                            "WebSocket send failed, client disconnected"
                        );
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!(
                        client_id = %client_id_for_outbound,
                        skipped = n,
                        "WebSocket broadcast lagged, messages dropped"
                    );
                }
                Err(broadcast::error::RecvError::Closed) => {
                    debug!(
                        client_id = %client_id_for_outbound,
                        "Broadcast channel closed"
                    );
                    break;
                }
            }
        }
    });

    // Task: process inbound messages from WebSocket
    let client_id_for_inbound = client_id_str.clone();
    let mut inbound_handle = tokio::spawn(async move {
        while let Some(msg_result) = ws_rx.next().await {
            let msg = match msg_result {
                Ok(m) => m,
                Err(e) => {
                    debug!(
                        client_id = %client_id_for_inbound,
                        error = %e,
                        "WebSocket receive error"
                    );
                    break;
                }
            };

            match msg {
                Message::Text(text) => {
                    handle_authenticated_message(&text, &client_id_for_inbound, &state_for_inbound)
                        .await;
                }
                Message::Close(_) => {
                    info!(
                        client_id = %client_id_for_inbound,
                        "WebSocket client sent close"
                    );
                    break;
                }
                Message::Ping(_) | Message::Pong(_) => {
                    // Axum handles ping/pong automatically
                }
                _ => {}
            }
        }
    });

    // Wait for either task to finish (disconnection from either side),
    // then abort the other to clean up the broadcast receiver.
    tokio::select! {
        _ = &mut outbound_handle => {
            inbound_handle.abort();
        },
        _ = &mut inbound_handle => {
            outbound_handle.abort();
        },
    }

    // Clean up
    state.registry.unregister(&client_id_str);
    info!(client_id = %client_id_str, "WebSocket connection closed");
}

/// Phase 1a: Wait for `auth_init` message, validate client_id, look up
/// client public key from Valkey cache.
///
/// The client sends `auth_init { client_id, kid }`. The server looks up
/// the public key from the Valkey client key cache (populated by the
/// state-snapshot consumer). The `kid` identifies which key the client
/// is using. If no `kid` is provided, the first available key is used.
async fn await_auth_init(
    ws_rx: &mut (impl StreamExt<Item = Result<Message, axum::Error>> + Unpin),
    _ws_tx: &mut (impl SinkExt<Message> + Unpin),
    state: &SharedWsState,
) -> Result<(String, PublicKey), String> {
    let msg = ws_rx
        .next()
        .await
        .ok_or_else(|| "connection closed before auth".to_string())?
        .map_err(|e| format!("WebSocket error: {}", e))?;

    let text = match msg {
        Message::Text(t) => t,
        _ => return Err("expected text message for auth_init".to_string()),
    };

    let inbound: WsInbound =
        serde_json::from_str(&text).map_err(|e| format!("invalid auth_init message: {}", e))?;

    let (client_id_str, kid) = match inbound {
        WsInbound::AuthInit(init) => (init.client_id, init.kid),
        _ => return Err("expected auth_init message".to_string()),
    };

    // Look up client public key from Valkey cache
    let client_id =
        ClientId::new(&client_id_str).map_err(|e| format!("invalid client_id: {}", e))?;

    let key_data = if let Some(ref kid) = kid {
        // Look up specific key by client_id + kid
        state
            .key_repo
            .find_key(&client_id, kid)
            .await
            .map_err(|e| format!("key lookup failed: {}", e))?
            .ok_or_else(|| {
                format!(
                    "no key found for client {} with kid {}",
                    client_id_str, kid
                )
            })?
    } else {
        // No kid specified — use the first available key
        let keys = state
            .key_repo
            .find_all_keys(&client_id)
            .await
            .map_err(|e| format!("key lookup failed: {}", e))?;
        keys.into_iter()
            .next()
            .ok_or_else(|| format!("no keys found for client {}", client_id_str))?
    };

    let client_pk = ec_public_key_from_device_state(&key_data)
        .map_err(|e| format!("invalid device public key: {}", e))?;

    Ok((client_id_str, client_pk))
}

/// Phase 1b: Wait for `auth_response` and verify HPKE proof.
///
/// The client's response must contain `HMAC-SHA256(key=nonce, msg=salt)`,
/// encrypted with HPKE AuthEncap(client_sk, server_pk). This proves the
/// client decrypted the nonce and combined it with the correct salt.
async fn await_auth_response(
    ws_rx: &mut (impl StreamExt<Item = Result<Message, axum::Error>> + Unpin),
    state: &SharedWsState,
    client_pk: &PublicKey,
    expected_nonce: &[u8],
    salt: &[u8],
) -> Result<(), String> {
    let msg = ws_rx
        .next()
        .await
        .ok_or_else(|| "connection closed before auth_response".to_string())?
        .map_err(|e| format!("WebSocket error: {}", e))?;

    let text = match msg {
        Message::Text(t) => t,
        _ => return Err("expected text message for auth_response".to_string()),
    };

    let inbound: WsInbound =
        serde_json::from_str(&text).map_err(|e| format!("invalid auth_response message: {}", e))?;

    let (enc_b64, ct_b64) = match inbound {
        WsInbound::AuthResponse(resp) => (resp.enc, resp.ciphertext),
        _ => return Err("expected auth_response message".to_string()),
    };

    let enc_bytes = b64_decode(&enc_b64).map_err(|e| format!("invalid enc: {}", e))?;
    let ct_bytes = b64_decode(&ct_b64).map_err(|e| format!("invalid ciphertext: {}", e))?;

    state
        .hpke_auth
        .verify_response(client_pk, &enc_bytes, &ct_bytes, expected_nonce, salt)
        .map_err(|e| format!("HPKE auth verification failed: {}", e))?;

    Ok(())
}

/// Handle an authenticated inbound message (service request).
async fn handle_authenticated_message(text: &str, client_id: &str, state: &SharedWsState) {
    let inbound: WsInbound = match serde_json::from_str(text) {
        Ok(msg) => msg,
        Err(e) => {
            warn!(
                client_id = %client_id,
                error = %e,
                "invalid WebSocket message"
            );
            return;
        }
    };

    match inbound {
        WsInbound::Request(req) => {
            handle_request(client_id, req, state).await;
        }
        _ => {
            warn!(
                client_id = %client_id,
                "unexpected message type in authenticated session"
            );
        }
    }
}

/// Process a service request received over WebSocket.
///
/// Equivalent to POST / but using the authenticated client_id and
/// always in async mode (responses are pushed via broadcast).
async fn handle_request(client_id: &str, req: WsRequestMsg, state: &SharedWsState) {
    let result = state
        .submit_request_use_case
        .execute(
            client_id,
            &req.outer_request_jws,
            Some(req.request_id.clone()),
            ProcessingMode::Asynchronous,
        )
        .await;

    match result {
        Ok(submit_result) => {
            let correlation_id = match &submit_result {
                crate::application::request_processing::SubmitResult::Pending {
                    correlation_id,
                } => *correlation_id,
                crate::application::request_processing::SubmitResult::Complete {
                    correlation_id,
                    ..
                } => *correlation_id,
                crate::application::request_processing::SubmitResult::Failed {
                    correlation_id,
                    ..
                } => *correlation_id,
            };

            debug!(
                client_id = %client_id,
                request_id = %req.request_id,
                correlation_id = %correlation_id,
                "WebSocket request submitted"
            );
        }
        Err(e) => {
            warn!(
                client_id = %client_id,
                request_id = %req.request_id,
                error = %e,
                "WebSocket request submission failed"
            );
        }
    }
}

/// Send a JSON-serialized message over the WebSocket.
async fn send_json(
    ws_tx: &mut (impl SinkExt<Message> + Unpin),
    msg: &WsOutbound,
) -> Result<(), ()> {
    let json = serde_json::to_string(msg).map_err(|_| ())?;
    ws_tx.send(Message::Text(json)).await.map_err(|_| ())?;
    Ok(())
}
