//! Redacted formatting for generated write-only values and bearer capabilities.
//!
//! The schema generator derives `Debug` indiscriminately. `tools/postprocess-generated.py`
//! removes those derives for the types below after generation; keeping these implementations in
//! handwritten source makes a missed postprocess fail at compile time instead of leaking a value.

use std::fmt;

macro_rules! redacted_debug {
    ($type:path, $name:literal) => {
        impl fmt::Debug for $type {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(concat!($name, "(<redacted>)"))
            }
        }
    };
}

redacted_debug!(
    crate::environment::BundleFetchHeadersValue,
    "BundleFetchHeadersValue"
);
redacted_debug!(crate::environment::BundleFetchUrl, "BundleFetchUrl");
redacted_debug!(crate::environment::BundleFetch, "BundleFetch");
redacted_debug!(
    crate::environment::ObjectTransferAuthorityHeadersValue,
    "ObjectTransferAuthorityHeadersValue"
);
redacted_debug!(
    crate::environment::ObjectTransferAuthorityUrl,
    "ObjectTransferAuthorityUrl"
);
redacted_debug!(
    crate::environment::ObjectTransferAuthority,
    "ObjectTransferAuthority"
);
redacted_debug!(
    crate::environment::PrepareSessionRequest,
    "PrepareSessionRequest"
);
redacted_debug!(crate::environment::SandboxCopyRequest, "SandboxCopyRequest");
redacted_debug!(
    crate::environment::SandboxFileWriteRequest,
    "SandboxFileWriteRequest"
);
redacted_debug!(
    crate::environment::SandboxFileWriteSource,
    "SandboxFileWriteSource"
);
redacted_debug!(crate::environment::SecretCapability, "SecretCapability");
redacted_debug!(
    crate::environment::SecretDeliveryRequest,
    "SecretDeliveryRequest"
);

redacted_debug!(crate::session::CreateSessionRequest, "CreateSessionRequest");
redacted_debug!(crate::session::ModelConfigApiKey, "ModelConfigApiKey");
redacted_debug!(crate::session::ModelConfig, "ModelConfig");
redacted_debug!(
    crate::session::ToolArtifactLayerContentBase64,
    "ToolArtifactLayerContentBase64"
);
redacted_debug!(crate::session::ToolArtifactLayer, "ToolArtifactLayer");
redacted_debug!(crate::session::ToolBundle, "ToolBundle");
