/// What the HTTP boundary accepts from a caller. This faces untrusted clients, so the
/// default is on; the deployment sets the size. Zero means no bound.
#[derive(Clone, Debug, clap::Args)]
pub struct HttpLimits {
    /// Bytes one HTTP request body may hold, including an admitted Component package.
    #[arg(long, env = "BRAIN_MAX_REQUEST_BYTES", default_value_t = HttpLimits::default().max_request_bytes)]
    pub max_request_bytes: usize,
}

impl Default for HttpLimits {
    fn default() -> Self {
        Self {
            max_request_bytes: 32 * 1024 * 1024,
        }
    }
}

impl HttpLimits {
    pub(crate) fn body_limit(&self) -> usize {
        if self.max_request_bytes == 0 {
            usize::MAX
        } else {
            self.max_request_bytes
        }
    }
}
