//! Customer Environment delivery through an AWS API Gateway WebSocket Management endpoint.

use async_trait::async_trait;
use aws_sdk_apigatewaymanagement::error::SdkError;
use aws_sdk_apigatewaymanagement::primitives::Blob;
use brain::customer::{CustomerDelivery, CustomerDeliveryRequest, CustomerEnvironmentDeliveryPort};

pub struct ApiGatewayCustomerDelivery {
    client: aws_sdk_apigatewaymanagement::Client,
}

impl ApiGatewayCustomerDelivery {
    pub async fn new(region: &str, callback_url: &str) -> anyhow::Result<Self> {
        validate_callback_url(callback_url)?;
        let shared = aws_config::from_env()
            .region(aws_config::Region::new(region.to_owned()))
            .load()
            .await;
        let config = aws_sdk_apigatewaymanagement::config::Builder::from(&shared)
            .endpoint_url(callback_url)
            .build();
        Ok(Self {
            client: aws_sdk_apigatewaymanagement::Client::from_conf(config),
        })
    }
}

#[async_trait]
impl CustomerEnvironmentDeliveryPort for ApiGatewayCustomerDelivery {
    async fn send(&self, request: CustomerDeliveryRequest) -> brain::Result<CustomerDelivery> {
        let frame = request.command.to_frame()?;
        match self
            .client
            .post_to_connection()
            .connection_id(request.connection_id)
            .data(Blob::new(frame))
            .send()
            .await
        {
            Ok(_) => Ok(CustomerDelivery::Delivered),
            Err(error)
                if error
                    .as_service_error()
                    .is_some_and(|service| service.is_gone_exception()) =>
            {
                Ok(CustomerDelivery::Gone)
            }
            Err(SdkError::ServiceError(error))
                if error.err().is_forbidden_exception()
                    || error.err().is_limit_exceeded_exception()
                    || error.err().is_payload_too_large_exception() =>
            {
                tracing::warn!(error = %error.err(), "customer Environment gateway rejected delivery");
                Ok(CustomerDelivery::Unavailable)
            }
            Err(SdkError::ServiceError(error)) => {
                tracing::warn!(error = %error.err(), "customer Environment delivery outcome is unknown");
                Ok(CustomerDelivery::Unknown)
            }
            Err(SdkError::ConstructionFailure(error)) => {
                tracing::warn!(error = ?error, "customer Environment delivery was not constructed");
                Ok(CustomerDelivery::Unavailable)
            }
            Err(error) => {
                tracing::warn!(error = %error, "customer Environment delivery outcome is unknown");
                Ok(CustomerDelivery::Unknown)
            }
        }
    }
}

fn validate_callback_url(value: &str) -> anyhow::Result<()> {
    let url = reqwest::Url::parse(value)
        .map_err(|error| anyhow::anyhow!("customer Environment callback URL: {error}"))?;
    if url.scheme() != "https" {
        anyhow::bail!("customer Environment callback URL must use HTTPS");
    }
    if url.host_str().is_none() {
        anyhow::bail!("customer Environment callback URL must have a host");
    }
    if !url.username().is_empty() || url.password().is_some() {
        anyhow::bail!("customer Environment callback URL must not contain credentials");
    }
    if url.query().is_some() || url.fragment().is_some() {
        anyhow::bail!("customer Environment callback URL must not contain a query or fragment");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_callback_url;

    #[test]
    fn callback_is_an_https_management_endpoint_without_embedded_credentials() {
        assert!(
            validate_callback_url("https://abc.execute-api.us-east-1.amazonaws.com/production")
                .is_ok()
        );
        assert!(validate_callback_url("http://localhost:3000/dev").is_err());
        assert!(validate_callback_url("https://user:secret@example.test/dev").is_err());
        assert!(validate_callback_url("https://example.test/dev?token=nope").is_err());
    }
}
