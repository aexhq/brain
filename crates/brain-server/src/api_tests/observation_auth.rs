use super::customer_ws::{
    bearer_token, customer_gateway_response, customer_grant_subprotocol, observation_grant_header,
};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};

#[test]
fn internal_observation_keeps_operator_and_scoped_grant_separate() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer operator-secret"),
    );
    headers.insert(
        "x-brain-observation-grant",
        HeaderValue::from_static("scoped-observation-secret"),
    );

    assert_eq!(bearer_token(&headers), Some("operator-secret"));
    assert_eq!(
        observation_grant_header(&headers),
        Some("scoped-observation-secret")
    );
}

#[test]
fn missing_or_swapped_internal_observation_authorities_do_not_match() {
    let missing = HeaderMap::new();
    assert_eq!(bearer_token(&missing), None);
    assert_eq!(observation_grant_header(&missing), None);

    let mut swapped = HeaderMap::new();
    swapped.insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer scoped-observation-secret"),
    );
    swapped.insert(
        "x-brain-observation-grant",
        HeaderValue::from_static("operator-secret"),
    );
    assert_ne!(bearer_token(&swapped), Some("operator-secret"));
    assert_ne!(
        observation_grant_header(&swapped),
        Some("scoped-observation-secret")
    );
}

#[test]
fn customer_socket_accepts_exactly_one_grant_subprotocol() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::SEC_WEBSOCKET_PROTOCOL,
        HeaderValue::from_static("environment-grant.valid-token"),
    );
    assert_eq!(
        customer_grant_subprotocol(&headers).unwrap(),
        "environment-grant.valid-token"
    );
}

#[test]
fn customer_socket_rejects_missing_extra_or_duplicate_subprotocols() {
    assert!(customer_grant_subprotocol(&HeaderMap::new()).is_err());

    let mut extra = HeaderMap::new();
    extra.insert(
        header::SEC_WEBSOCKET_PROTOCOL,
        HeaderValue::from_static("environment-grant.valid-token, chat"),
    );
    assert!(customer_grant_subprotocol(&extra).is_err());

    let mut duplicate = HeaderMap::new();
    duplicate.append(
        header::SEC_WEBSOCKET_PROTOCOL,
        HeaderValue::from_static("environment-grant.first"),
    );
    duplicate.append(
        header::SEC_WEBSOCKET_PROTOCOL,
        HeaderValue::from_static("environment-grant.second"),
    );
    assert!(customer_grant_subprotocol(&duplicate).is_err());

    let mut empty_grant = HeaderMap::new();
    empty_grant.insert(
        header::SEC_WEBSOCKET_PROTOCOL,
        HeaderValue::from_static("environment-grant."),
    );
    assert!(customer_grant_subprotocol(&empty_grant).is_err());
}

#[test]
fn customer_gateway_connect_acknowledges_upgrade_and_message_acknowledges_delivery() {
    let protocol = "environment-grant.valid-token";
    let connect = customer_gateway_response(
        brain::customer::CustomerGatewayRoute::Connect,
        Some(protocol.into()),
    )
    .unwrap();
    assert_eq!(connect.status(), StatusCode::OK);
    assert_eq!(
        connect.headers().get(header::SEC_WEBSOCKET_PROTOCOL),
        Some(&HeaderValue::from_static(protocol))
    );

    let message =
        customer_gateway_response(brain::customer::CustomerGatewayRoute::Message, None).unwrap();
    assert_eq!(message.status(), StatusCode::NO_CONTENT);
    assert!(
        !message
            .headers()
            .contains_key(header::SEC_WEBSOCKET_PROTOCOL)
    );
}
