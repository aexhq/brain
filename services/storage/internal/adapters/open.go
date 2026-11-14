package adapters

// Backend configuration is constructed in the service composition root. Keeping
// constructors out of this package avoids an adapter-registry import cycle.
type Config struct {
	Kind         string `json:"kind"`
	Path         string `json:"path,omitempty"`
	Bucket       string `json:"bucket,omitempty"`
	Table        string `json:"table,omitempty"`
	Region       string `json:"region,omitempty"`
	Endpoint     string `json:"endpoint,omitempty"`
	Prefix       string `json:"prefix,omitempty"`
	PathStyle    bool   `json:"path_style,omitempty"`
	DSNEnv       string `json:"dsn_env,omitempty"`
	AccessKeyEnv string `json:"access_key_env,omitempty"`
	SecretKeyEnv string `json:"secret_key_env,omitempty"`
}
