use super::*;

pub(super) async fn operator_auth_before_body(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if let Err(failure) = auth(&state, request.headers()) {
        return failure.into_response();
    }
    next.run(request).await
}

pub(super) async fn create_admission_before_body(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let permit = match state.brain.try_admit_create() {
        Ok(permit) => permit,
        Err(error) => return map_err(error).into_response(),
    };
    let response = next.run(request).await;
    drop(permit);
    response
}

fn observation_grant_id(request: &Request) -> Option<String> {
    request
        .uri()
        .path()
        .rsplit_once('/')
        .map(|(_, grant_id)| grant_id)
        .filter(|grant_id| !grant_id.is_empty())
        .map(str::to_owned)
}

async fn authorize_observation_before_body(
    state: &AppState,
    grant_id: Option<String>,
    token: Option<String>,
) -> Result<(), Failure> {
    let grant_id = grant_id.ok_or_else(invalid_observation_grant)?;
    let token = token.ok_or_else(invalid_observation_grant)?;
    let coordinator = state.brain.customer.as_ref().ok_or_else(|| {
        map_err(BrainError::HandUnavailable(
            "customer Hand is unavailable".into(),
        ))
    })?;
    coordinator
        .authorize_observation(&grant_id, &token)
        .await
        .map_err(|_| invalid_observation_grant())
}

pub(super) async fn public_observation_auth_before_body(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let grant_id = observation_grant_id(&request);
    let token = bearer_token(request.headers()).map(str::to_owned);
    if let Err(failure) = authorize_observation_before_body(&state, grant_id, token).await {
        return failure.into_response();
    }
    next.run(request).await
}

pub(super) async fn internal_observation_auth_before_body(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if let Err(failure) = auth(&state, request.headers()) {
        return failure.into_response();
    }
    let grant_id = observation_grant_id(&request);
    let token = observation_grant_header(request.headers()).map(str::to_owned);
    if let Err(failure) = authorize_observation_before_body(&state, grant_id, token).await {
        return failure.into_response();
    }
    next.run(request).await
}

#[derive(Deserialize)]
pub(super) struct CustomerGrantRequest {
    client_id: String,
}

#[derive(Serialize)]
pub(super) struct CustomerGrantResponse {
    url: String,
    protocol: String,
    expires_at: brain_protocol::session::Timestamp,
    grant_id: String,
    observation_url: String,
    observation_token: String,
}

pub(super) async fn customer_hand_grant(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CustomerGrantRequest>,
) -> Result<Json<CustomerGrantResponse>, Failure> {
    let principal = auth(&state, &headers)?;
    let coordinator = state.brain.customer.as_ref().ok_or_else(|| {
        Failure(
            StatusCode::SERVICE_UNAVAILABLE,
            api_code("service_unavailable"),
            "customer-app Tools are unavailable in this composition".into(),
        )
    })?;
    let grant = coordinator
        .grant(principal.as_str(), &request.client_id)
        .await
        .map_err(map_err)?;
    Ok(Json(CustomerGrantResponse {
        url: grant.url,
        protocol: grant.protocol,
        expires_at: crate::events::ts(grant.expires_at_ms),
        grant_id: grant.grant_id,
        observation_url: grant.observation_url,
        observation_token: grant.observation_token,
    }))
}

pub(super) async fn customer_hand_socket(
    State(state): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, Failure> {
    let coordinator = state.brain.customer.clone().ok_or_else(|| {
        Failure(
            StatusCode::SERVICE_UNAVAILABLE,
            api_code("service_unavailable"),
            "customer-app Tools are unavailable in this composition".into(),
        )
    })?;
    let protocol = customer_grant_subprotocol(&headers)?;
    let connection_id = mint_id("conn", 24);
    crate::customer::CustomerHandIngressPort::receive(
        coordinator.as_ref(),
        crate::customer::CustomerGatewayInput {
            route: crate::customer::CustomerGatewayRoute::Connect,
            connection_id: connection_id.clone(),
            request_id: mint_id("req", 16),
            route_key: "$connect".into(),
            source_ip: "127.0.0.1".into(),
            subprotocol: Some(protocol.clone()),
            body: None,
        },
    )
    .await
    .map_err(map_err)?;
    Ok(ws
        .protocols([protocol])
        .max_message_size(crate::customer::MAX_CUSTOMER_WS_FRAME_BYTES)
        .max_frame_size(crate::customer::MAX_CUSTOMER_WS_FRAME_BYTES)
        .on_upgrade(move |socket| serve_customer_hand_socket(coordinator, connection_id, socket))
        .into_response())
}

async fn serve_customer_hand_socket(
    coordinator: Arc<crate::customer::CustomerCoordinator>,
    connection_id: String,
    socket: WebSocket,
) {
    let (mut sink, mut source) = socket.split();
    let (sender, mut outbound) = tokio::sync::mpsc::channel(128);
    if coordinator
        .bind_local_sender(&connection_id, sender)
        .await
        .is_err()
    {
        let _ = sink.close().await;
        return;
    }
    loop {
        tokio::select! {
            frame = outbound.recv() => {
                let Some(frame) = frame else { break; };
                let Ok(bytes) = frame.to_frame() else { break; };
                let Ok(text) = String::from_utf8(bytes) else { break; };
                if sink.send(WsMessage::Text(text.into())).await.is_err() { break; }
            }
            frame = source.next() => {
                let Some(Ok(frame)) = frame else { break; };
                match frame {
                    WsMessage::Text(text) => {
                        let result = crate::customer::CustomerHandIngressPort::receive(
                            coordinator.as_ref(),
                            crate::customer::CustomerGatewayInput {
                                route: crate::customer::CustomerGatewayRoute::Message,
                                connection_id: connection_id.clone(),
                                request_id: mint_id("req", 16),
                                route_key: "$default".into(),
                                source_ip: "127.0.0.1".into(),
                                subprotocol: None,
                                body: Some(text.to_string()),
                            },
                        ).await;
                        if result.is_err() { break; }
                    }
                    WsMessage::Ping(bytes) => {
                        if sink.send(WsMessage::Pong(bytes)).await.is_err() { break; }
                    }
                    WsMessage::Close(_) => break,
                    WsMessage::Binary(_) | WsMessage::Pong(_) => {}
                }
            }
        }
    }
    let _ = crate::customer::CustomerHandIngressPort::receive(
        coordinator.as_ref(),
        crate::customer::CustomerGatewayInput {
            route: crate::customer::CustomerGatewayRoute::Disconnect,
            connection_id,
            request_id: mint_id("req", 16),
            route_key: "$disconnect".into(),
            source_ip: "127.0.0.1".into(),
            subprotocol: None,
            body: None,
        },
    )
    .await;
}

pub(super) async fn customer_hand_observation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(grant_id): Path<String>,
    body: Bytes,
) -> Result<StatusCode, Failure> {
    let observation_token = bearer_token(&headers).ok_or_else(invalid_observation_grant)?;
    apply_customer_hand_observation(&state, &grant_id, observation_token, &body).await
}

pub(super) async fn internal_customer_hand_observation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(grant_id): Path<String>,
    body: Bytes,
) -> Result<StatusCode, Failure> {
    // Internal callers authenticate as the operator/service with Authorization. The scoped
    // customer observation grant is deliberately carried in a separate header so the two
    // authorities cannot be confused or substituted for one another.
    auth(&state, &headers)?;
    let observation_token =
        observation_grant_header(&headers).ok_or_else(invalid_observation_grant)?;
    apply_customer_hand_observation(&state, &grant_id, observation_token, &body).await
}

pub(super) fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty())
}

pub(super) fn observation_grant_header(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("x-brain-observation-grant")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
}

fn invalid_observation_grant() -> Failure {
    Failure(
        StatusCode::UNAUTHORIZED,
        api_code("unauthorized"),
        "invalid customer Hand observation grant".into(),
    )
}

async fn apply_customer_hand_observation(
    state: &AppState,
    grant_id: &str,
    observation_token: &str,
    body: &[u8],
) -> Result<StatusCode, Failure> {
    if body.len() > crate::customer::MAX_CUSTOMER_HTTP_OBSERVATION_BYTES {
        return Err(map_err(BrainError::FileTooLarge {
            limit: crate::customer::MAX_CUSTOMER_HTTP_OBSERVATION_BYTES,
        }));
    }
    let observation = serde_json::from_slice(body).map_err(|error| {
        Failure(
            StatusCode::BAD_REQUEST,
            api_code("invalid_request"),
            format!("customer Hand observation: {error}"),
        )
    })?;
    state
        .brain
        .customer
        .as_ref()
        .ok_or_else(|| {
            map_err(BrainError::HandUnavailable(
                "customer Hand is unavailable".into(),
            ))
        })?
        .observation(grant_id, observation_token, observation)
        .await
        .map_err(map_err)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn customer_hand_gateway(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, Failure> {
    auth(&state, &headers)?;
    let connection_id = trusted_header(&headers, "x-brain-connection-id")?;
    let request_id = trusted_header(&headers, "x-brain-request-id")?;
    let route_key = trusted_header(&headers, "x-brain-route-key")?;
    let source_ip = trusted_header(&headers, "x-brain-source-ip")?;
    let route = match route_key.as_str() {
        "$connect" => crate::customer::CustomerGatewayRoute::Connect,
        "$disconnect" => crate::customer::CustomerGatewayRoute::Disconnect,
        "$default" => crate::customer::CustomerGatewayRoute::Message,
        _ => {
            return Err(Failure(
                StatusCode::BAD_REQUEST,
                api_code("invalid_request"),
                "x-brain-route-key must be $connect, $disconnect, or $default".into(),
            ));
        }
    };
    if route == crate::customer::CustomerGatewayRoute::Message
        && body.len() > crate::customer::MAX_CUSTOMER_WS_FRAME_BYTES
    {
        return Err(map_err(BrainError::FileTooLarge {
            limit: crate::customer::MAX_CUSTOMER_WS_FRAME_BYTES,
        }));
    }
    let subprotocol = if route == crate::customer::CustomerGatewayRoute::Connect {
        Some(customer_grant_subprotocol(&headers)?)
    } else {
        None
    };
    let body = if route == crate::customer::CustomerGatewayRoute::Message {
        Some(String::from_utf8(body.to_vec()).map_err(|_| {
            Failure(
                StatusCode::BAD_REQUEST,
                api_code("invalid_request"),
                "customer Hand WebSocket frame must be UTF-8 text".into(),
            )
        })?)
    } else {
        None
    };
    let coordinator = state.brain.customer.as_ref().ok_or_else(|| {
        map_err(BrainError::HandUnavailable(
            "customer Hand is unavailable".into(),
        ))
    })?;
    crate::customer::CustomerHandIngressPort::receive(
        coordinator.as_ref(),
        crate::customer::CustomerGatewayInput {
            route,
            connection_id,
            request_id,
            route_key,
            source_ip,
            subprotocol: subprotocol.clone(),
            body,
        },
    )
    .await
    .map_err(map_err)?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    if let Some(protocol) = subprotocol {
        response.headers_mut().insert(
            header::SEC_WEBSOCKET_PROTOCOL,
            HeaderValue::from_str(&protocol).map_err(|_| {
                Failure(
                    StatusCode::BAD_REQUEST,
                    api_code("invalid_request"),
                    "customer Hand grant protocol is invalid".into(),
                )
            })?,
        );
    }
    Ok(response)
}

fn trusted_header(headers: &HeaderMap, name: &'static str) -> Result<String, Failure> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            Failure(
                StatusCode::BAD_REQUEST,
                api_code("invalid_request"),
                format!("trusted gateway header {name} is required"),
            )
        })
}

pub(super) fn customer_grant_subprotocol(headers: &HeaderMap) -> Result<String, Failure> {
    let invalid = || {
        Failure(
            StatusCode::UNAUTHORIZED,
            api_code("unauthorized"),
            "exactly one customer Hand grant subprotocol is required".into(),
        )
    };
    let mut protocol = None;
    for value in headers.get_all(header::SEC_WEBSOCKET_PROTOCOL) {
        let value = value.to_str().map_err(|_| invalid())?;
        for candidate in value.split(',').map(str::trim) {
            if candidate.len() <= "aex-grant.".len()
                || !candidate.starts_with("aex-grant.")
                || protocol.is_some()
            {
                return Err(invalid());
            }
            protocol = Some(candidate.to_owned());
        }
    }
    protocol.ok_or_else(invalid)
}
