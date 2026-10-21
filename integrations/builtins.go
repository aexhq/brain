package integrations

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/url"
	"os"
	"slices"
	"strings"

	"github.com/aexhq/brain"
	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/toolservice"
)

// Installation is the stock application's build-time integration configuration.
// Credentials are environment references; session overrides use Brain's private bindings.
type Installation struct {
	Name          string          `json:"name"`
	Parameters    json.RawMessage `json:"parameters"`
	Operations    []string        `json:"operations"`
	CredentialEnv string          `json:"credential_env,omitempty"`
	URL           string          `json:"url,omitempty"`
	TokenEnv      string          `json:"token_env,omitempty"`
	Tools         []brain.Tool    `json:"tools,omitempty"`
}

// LoadBuiltins installs configured integrations without requiring default credentials.
func LoadBuiltins(file string) (map[string]brain.ToolProvider, error) {
	f, err := os.Open(file)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	var definitions []Installation
	d := json.NewDecoder(io.LimitReader(f, 1<<20))
	d.DisallowUnknownFields()
	if err := d.Decode(&definitions); err != nil {
		return nil, err
	}
	if d.Decode(new(any)) != io.EOF {
		return nil, errors.New("trailing integration configuration")
	}
	providers := map[string]brain.ToolProvider{}
	for _, definition := range definitions {
		if _, exists := providers[definition.Name]; exists {
			return nil, fmt.Errorf("duplicate integration %s", definition.Name)
		}
		provider, err := builtin(definition)
		if err != nil {
			return nil, err
		}
		providers[definition.Name] = provider
	}
	return providers, nil
}

func objectFields(raw json.RawMessage, allowed []string) (map[string]string, error) {
	var values map[string]string
	if json.Unmarshal(raw, &values) != nil || values == nil {
		return nil, core.E(core.Invalid, "integration options must be an object of strings")
	}
	for field := range values {
		if !slices.Contains(allowed, field) {
			return nil, core.E(core.Invalid, "unknown integration option: "+field)
		}
	}
	return values, nil
}

func builtin(definition Installation) (brain.ToolProvider, error) {
	fields, secrets, required, operations := []string{}, []string{}, []string{}, []string{}
	switch definition.Name {
	case "aws_s3":
		fields = []string{"bucket", "region", "endpoint", "prefix"}
		secrets = []string{"accessKeyId", "secretAccessKey", "sessionToken"}
		required = []string{"accessKeyId", "secretAccessKey"}
		operations = []string{"list", "read", "write", "delete"}
	case "modal":
		fields, secrets = []string{"app", "image"}, []string{"tokenId", "tokenSecret"}
		required = secrets
	case "e2b":
		fields, secrets, required = []string{"template"}, []string{"apiKey"}, []string{"apiKey"}
	case "daytona":
		fields, secrets, required = []string{"apiUrl", "target"}, []string{"apiKey"}, []string{"apiKey"}
	default:
		return brain.ToolProvider{}, fmt.Errorf("unknown integration %s", definition.Name)
	}
	if definition.Name != "aws_s3" {
		operations = []string{"create", "exec", "inspect", "terminate"}
	}
	hostToken := os.Getenv(definition.TokenEnv)
	if definition.Name != "aws_s3" {
		u, err := url.Parse(definition.URL)
		if err != nil || (u.Scheme != "http" && u.Scheme != "https") || u.Host == "" || u.User != nil || u.Fragment != "" || len(hostToken) < 24 {
			return brain.ToolProvider{}, errors.New("sandbox tool host URL and credential are required")
		}
	}
	if len(definition.Operations) == 0 {
		return brain.ToolProvider{}, errors.New("integration operations are required")
	}
	seen := map[string]bool{}
	for _, operation := range definition.Operations {
		if !slices.Contains(operations, operation) || seen[operation] {
			return brain.ToolProvider{}, errors.New("invalid or duplicate integration operation")
		}
		seen[operation] = true
	}
	defaults, err := objectFields(definition.Parameters, fields)
	if err != nil {
		return brain.ToolProvider{}, err
	}
	var defaultCredential json.RawMessage
	if definition.CredentialEnv != "" {
		defaultCredential = json.RawMessage(os.Getenv(definition.CredentialEnv))
		if len(defaultCredential) == 0 {
			return brain.ToolProvider{}, errors.New("configured integration credential is unavailable")
		}
	}
	credential := func(override json.RawMessage) (map[string]string, error) {
		value := defaultCredential
		if override != nil {
			value = override
		}
		if len(value) == 0 {
			return nil, core.E(core.Forbidden, definition.Name+" credentials are required")
		}
		values, err := objectFields(value, secrets)
		if err != nil {
			return nil, core.E(core.Forbidden, "invalid "+definition.Name+" credentials")
		}
		for _, field := range required {
			if values[field] == "" {
				return nil, core.E(core.Forbidden, "invalid "+definition.Name+" credentials")
			}
		}
		return values, nil
	}
	if defaultCredential != nil {
		if _, err := credential(nil); err != nil {
			return brain.ToolProvider{}, err
		}
	}
	properties := map[string]any{}
	for _, field := range fields {
		properties[field] = map[string]string{"type": "string"}
	}
	schema, _ := json.Marshal(map[string]any{"type": "object", "properties": properties, "additionalProperties": false})
	capacity := make(chan struct{}, 32)
	return brain.ToolProvider{
		Parameters: schema,
		// An installed integration is available to authenticated tenants. Resource
		// changes below require their own credentials; endpoints remain host-owned.
		Authorize: func(context.Context, brain.BindingContext) error { return nil },
		Bind: func(ctx context.Context, binding brain.BindingContext) ([]brain.ManagedTool, error) {
			selected, err := objectFields(binding.Parameters, fields)
			if err != nil {
				return nil, err
			}
			parameters := map[string]string{}
			for key, value := range defaults {
				parameters[key] = value
			}
			for key, value := range selected {
				if value != defaults[key] && (key == "endpoint" || key == "apiUrl" || (defaults[key] != "" && binding.Credential == nil)) {
					return nil, core.E(core.Forbidden, "resource override requires private credentials and an installed endpoint")
				}
				parameters[key] = value
			}
			secret, err := credential(binding.Credential)
			if err != nil {
				return nil, err
			}
			if definition.Name == "aws_s3" {
				if parameters["bucket"] == "" || parameters["region"] == "" {
					return nil, core.E(core.Invalid, "S3 bucket and region are required")
				}
				service, err := toolservice.New(ctx, toolservice.Settings{
					Token: core.ID(), Bucket: parameters["bucket"], Region: parameters["region"], Endpoint: parameters["endpoint"], Prefix: parameters["prefix"],
					AccessKey: secret["accessKeyId"], SecretKey: secret["secretAccessKey"], SessionToken: secret["sessionToken"],
					AllowWrite: seen["write"] || seen["delete"],
					Capacity:   capacity,
				})
				if err != nil {
					return nil, err
				}
				result := []brain.ManagedTool{}
				for _, tool := range service.Catalog("http://unused", binding.Tenant) {
					operation, storage := strings.CutPrefix(tool.Name, "storage_")
					if !storage || !seen[operation] {
						continue
					}
					path := "/" + tool.Name
					tool.Name, tool.URL, tool.TokenEnv = operation, "", ""
					tool.Handler = func(ctx context.Context, request brain.ToolRequest, _ func(json.RawMessage)) brain.Result {
						defer service.Close()
						return service.Call(ctx, path, request)
					}
					result = append(result, tool)
				}
				return result, nil
			}
			private, _ := json.Marshal(map[string]any{"provider": definition.Name, "parameters": parameters, "credential": secret})
			result := []brain.ManagedTool{}
			for _, tool := range definition.Tools {
				if !seen[tool.Name] {
					continue
				}
				result = append(result, brain.ManagedTool{Tool: tool, URL: strings.TrimRight(definition.URL, "/") + "/" + tool.Name,
					Credential: hostToken, Context: bytes.Clone(private), Tenants: []string{binding.Tenant}, TimeoutSeconds: 90})
			}
			if len(result) != len(definition.Operations) {
				return nil, errors.New("sandbox integration descriptors are incomplete")
			}
			return result, nil
		},
	}, nil
}
