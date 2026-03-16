use axum::{
    extract::{State, WebSocketUpgrade},
    response::IntoResponse,
};
use tracing::info;

use super::connection::handle_connection;
use super::state::SharedWsState;

/// WebSocket upgrade handler.
///
/// Route: GET /ws (under context_path)
///
/// Upgrades an HTTP connection to a WebSocket. The connection then
/// proceeds through HPKE mutual authentication before accepting
/// service requests.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<SharedWsState>,
) -> impl IntoResponse {
    info!("WebSocket upgrade request received");
    ws.on_upgrade(move |socket| handle_connection(socket, state))
}
