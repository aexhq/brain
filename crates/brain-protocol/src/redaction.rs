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
    crate::hand::BundleFetchHeadersValue,
    "BundleFetchHeadersValue"
);
redacted_debug!(crate::hand::BundleFetchUrl, "BundleFetchUrl");
redacted_debug!(crate::hand::BundleFetch, "BundleFetch");
redacted_debug!(
    crate::hand::ObjectTransferAuthorityHeadersValue,
    "ObjectTransferAuthorityHeadersValue"
);
redacted_debug!(
    crate::hand::ObjectTransferAuthorityUrl,
    "ObjectTransferAuthorityUrl"
);
redacted_debug!(
    crate::hand::ObjectTransferAuthority,
    "ObjectTransferAuthority"
);
redacted_debug!(crate::hand::PrepareSessionRequest, "PrepareSessionRequest");
redacted_debug!(crate::hand::SandboxCopyRequest, "SandboxCopyRequest");
redacted_debug!(
    crate::hand::SandboxFileWriteRequest,
    "SandboxFileWriteRequest"
);
redacted_debug!(
    crate::hand::SandboxFileWriteSource,
    "SandboxFileWriteSource"
);
redacted_debug!(crate::hand::SecretCapability, "SecretCapability");
redacted_debug!(crate::hand::SecretDeliveryRequest, "SecretDeliveryRequest");

redacted_debug!(crate::session::CreateSessionRequest, "CreateSessionRequest");
redacted_debug!(crate::session::ModelConfigApiKey, "ModelConfigApiKey");
redacted_debug!(crate::session::ModelConfig, "ModelConfig");
redacted_debug!(
    crate::session::ToolBundleContentBase64,
    "ToolBundleContentBase64"
);
redacted_debug!(crate::session::ToolBundle, "ToolBundle");
