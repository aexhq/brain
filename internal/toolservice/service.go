// Package toolservice implements optional external tools, outside the runtime.
package toolservice

import (
	"context"
	"crypto/subtle"
	"encoding/base64"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/url"
	"os"
	"path"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/aexhq/brain/internal/core"
	"github.com/aexhq/brain/internal/tools"
	"github.com/aws/aws-sdk-go-v2/aws"
	awsconfig "github.com/aws/aws-sdk-go-v2/config"
	"github.com/aws/aws-sdk-go-v2/credentials"
	"github.com/aws/aws-sdk-go-v2/service/s3"
)

type Settings struct {
	Token, SearchProvider, SearchKey                       string
	Bucket, Region, Endpoint, Prefix, AccessKey, SecretKey string
	AllowWrite                                             bool
}

type Service struct {
	cfg           Settings
	objects       *s3.Client
	client, fetch *http.Client
	active        chan struct{}
}

func New(ctx context.Context, cfg Settings) (*Service, error) {
	if len(cfg.Token) < 24 {
		return nil, errors.New("TOOL_API_KEY requires at least 24 characters")
	}
	if cfg.SearchProvider != "" && cfg.SearchProvider != "brave" && cfg.SearchProvider != "tavily" {
		return nil, errors.New("SEARCH_PROVIDER must be brave or tavily")
	}
	if cfg.SearchProvider != "" && cfg.SearchKey == "" {
		return nil, errors.New("SEARCH_API_KEY is missing")
	}
	if cfg.Prefix != "" && !validKey(strings.TrimSuffix(cfg.Prefix, "/")) {
		return nil, errors.New("invalid storage prefix")
	}
	s := &Service{cfg: cfg, active: make(chan struct{}, 32), fetch: publicClient(), client: &http.Client{Timeout: 20 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return errors.New("redirect disabled") }}}
	if cfg.Bucket != "" {
		opts := []func(*awsconfig.LoadOptions) error{awsconfig.WithRegion(cfg.Region), awsconfig.WithRetryMaxAttempts(1)}
		if cfg.AccessKey != "" || cfg.SecretKey != "" {
			if cfg.AccessKey == "" || cfg.SecretKey == "" {
				return nil, errors.New("both storage credentials are required")
			}
			opts = append(opts, awsconfig.WithCredentialsProvider(credentials.NewStaticCredentialsProvider(cfg.AccessKey, cfg.SecretKey, "")))
		}
		if cfg.Endpoint != "" {
			u, err := url.Parse(cfg.Endpoint)
			if err != nil || u.Scheme != "https" || u.Host == "" || u.User != nil {
				return nil, errors.New("storage endpoint requires HTTPS")
			}
		}
		config, err := awsconfig.LoadDefaultConfig(ctx, opts...)
		if err != nil {
			return nil, err
		}
		s.objects = s3.NewFromConfig(config, func(o *s3.Options) {
			if cfg.Endpoint != "" {
				o.BaseEndpoint = &cfg.Endpoint
				o.UsePathStyle = true
			}
			o.RequestChecksumCalculation = aws.RequestChecksumCalculationWhenRequired
			o.ResponseChecksumValidation = aws.ResponseChecksumValidationWhenRequired
		})
	}
	return s, nil
}

func (s *Service) Handler() http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		if r.Method == "GET" && r.URL.Path == "/health" {
			_, _ = io.WriteString(w, `{"status":"ok"}`)
			return
		}
		if subtle.ConstantTimeCompare([]byte(r.Header.Get("Authorization")), []byte("Bearer "+s.cfg.Token)) != 1 {
			w.WriteHeader(401)
			return
		}
		if r.Method != "POST" {
			w.WriteHeader(405)
			return
		}
		select {
		case s.active <- struct{}{}:
			defer func() { <-s.active }()
		default:
			_ = json.NewEncoder(w).Encode(core.ToolError("tool service is at capacity; no action dispatched", false))
			return
		}
		var request tools.Request
		d := json.NewDecoder(http.MaxBytesReader(w, r.Body, 1<<20))
		d.DisallowUnknownFields()
		if err := d.Decode(&request); err != nil || d.Decode(new(any)) != io.EOF || request.Tenant == "" || request.Session == "" || request.Call == "" || len(request.Tenant) > 256 {
			w.WriteHeader(400)
			return
		}
		ctx, cancel := context.WithTimeout(r.Context(), 25*time.Second)
		defer cancel()
		result := s.call(ctx, r.URL.Path, request)
		_ = json.NewEncoder(w).Encode(result)
	})
}

func decode(raw json.RawMessage, target any) error {
	if !strings.HasPrefix(strings.TrimSpace(string(raw)), "{") {
		return errors.New("arguments must be an object")
	}
	d := json.NewDecoder(strings.NewReader(string(raw)))
	d.DisallowUnknownFields()
	if err := d.Decode(target); err != nil {
		return err
	}
	if d.Decode(new(any)) != io.EOF {
		return errors.New("trailing arguments")
	}
	return nil
}

func textResult(value any) core.Result {
	data, _ := json.Marshal(value)
	return core.Result{Content: []core.Content{{Type: "text", Text: string(data)}}}
}

func (s *Service) call(ctx context.Context, operation string, request tools.Request) core.Result {
	switch operation {
	case "/web_fetch":
		var args struct {
			URL string `json:"url"`
		}
		if decode(request.Arguments, &args) != nil || len(args.URL) > 4096 {
			return core.ToolError("invalid fetch arguments", false)
		}
		return s.webFetch(ctx, args.URL)
	case "/web_search":
		var args struct {
			Query string `json:"query"`
		}
		if decode(request.Arguments, &args) != nil || strings.TrimSpace(args.Query) == "" || len(args.Query) > 2048 {
			return core.ToolError("invalid search arguments", false)
		}
		return s.webSearch(ctx, args.Query)
	case "/storage_read", "/storage_write", "/storage_delete", "/storage_list":
		return s.storage(ctx, operation, request)
	default:
		return core.ToolError("unknown tool", false)
	}
}

func validKey(key string) bool {
	return key != "" && path.Clean(key) == key && !strings.HasPrefix(key, "/") && !strings.Contains(key, "\\") && key != ".." && !strings.HasPrefix(key, "../")
}

func (s *Service) storage(ctx context.Context, op string, req tools.Request) core.Result {
	if s.objects == nil {
		return core.ToolError("storage is not configured", false)
	}
	var args struct {
		Key    string  `json:"key,omitempty"`
		Text   *string `json:"text,omitempty"`
		Cursor string  `json:"cursor,omitempty"`
	}
	if decode(req.Arguments, &args) != nil {
		return core.ToolError("invalid storage arguments", false)
	}
	if op != "/storage_list" && (!validKey(args.Key) || len(args.Key) > 512) {
		return core.ToolError("invalid object key", false)
	}
	if (op != "/storage_write" && args.Text != nil) || (op != "/storage_list" && args.Cursor != "") || (op == "/storage_list" && args.Key != "") {
		return core.ToolError("unexpected storage argument", false)
	}
	if (op == "/storage_write" || op == "/storage_delete") && !s.cfg.AllowWrite {
		return core.ToolError("storage mutations are disabled", false)
	}
	prefix := strings.Trim(s.cfg.Prefix, "/")
	if prefix != "" {
		prefix += "/"
	}
	prefix += base64.RawURLEncoding.EncodeToString([]byte(req.Tenant)) + "/"
	key := prefix + args.Key
	switch op {
	case "/storage_list":
		input := &s3.ListObjectsV2Input{Bucket: &s.cfg.Bucket, Prefix: &prefix, MaxKeys: aws.Int32(100)}
		if args.Cursor != "" {
			input.ContinuationToken = &args.Cursor
		}
		out, err := s.objects.ListObjectsV2(ctx, input)
		if err != nil {
			return core.ToolError("object listing failed", false)
		}
		keys := []string{}
		for _, object := range out.Contents {
			if object.Key != nil && strings.HasPrefix(*object.Key, prefix) {
				keys = append(keys, strings.TrimPrefix(*object.Key, prefix))
			}
		}
		return textResult(map[string]any{"keys": keys, "cursor": aws.ToString(out.NextContinuationToken)})
	case "/storage_read":
		out, err := s.objects.GetObject(ctx, &s3.GetObjectInput{Bucket: &s.cfg.Bucket, Key: &key})
		if err != nil {
			return core.ToolError("object read failed or object missing", false)
		}
		defer out.Body.Close()
		data, err := io.ReadAll(io.LimitReader(out.Body, (128<<10)+1))
		if err != nil || len(data) > 128<<10 || !utf8.Valid(data) {
			return core.ToolError("object must be UTF-8 text under 128 KiB", false)
		}
		return textResult(map[string]string{"key": args.Key, "text": string(data)})
	case "/storage_write":
		if args.Text == nil || len(*args.Text) > 128<<10 {
			return core.ToolError("text is required and limited to 128 KiB", false)
		}
		_, err := s.objects.PutObject(ctx, &s3.PutObjectInput{Bucket: &s.cfg.Bucket, Key: &key, Body: strings.NewReader(*args.Text), ContentType: aws.String("text/plain; charset=utf-8")})
		if err != nil {
			return core.ToolError("object write failed; remote outcome may be unknown", true)
		}
	case "/storage_delete":
		_, err := s.objects.DeleteObject(ctx, &s3.DeleteObjectInput{Bucket: &s.cfg.Bucket, Key: &key})
		if err != nil {
			return core.ToolError("object delete failed; remote outcome may be unknown", true)
		}
	}
	return textResult(map[string]any{"key": args.Key, "ok": true})
}

func (s *Service) Catalog(base, tenant string) []tools.Binding {
	type property map[string]any
	makeTool := func(name, description string, properties map[string]property, required []string) tools.Binding {
		schema, _ := json.Marshal(map[string]any{"type": "object", "properties": properties, "required": required, "additionalProperties": false})
		return tools.Binding{Tool: core.Tool{Name: name, Description: description, InputSchema: schema}, URL: strings.TrimRight(base, "/") + "/" + name, TokenEnv: "RESOURCE_TOOL_TOKEN", Tenants: []string{tenant}, TimeoutSeconds: 30, MaxOutputBytes: 2 << 20}
	}
	result := []tools.Binding{makeTool("web_fetch", "Fetch text from a public HTTP(S) page. Private networks are blocked; content is limited to 2 MiB.", map[string]property{"url": {"type": "string"}}, []string{"url"})}
	if s.cfg.SearchProvider != "" {
		result = append(result, makeTool("web_search", "Search the web with "+s.cfg.SearchProvider+". Results include source URLs.", map[string]property{"query": {"type": "string", "maxLength": 2048}}, []string{"query"}))
	}
	if s.objects != nil {
		result = append(result, makeTool("storage_list", "List this tenant's text objects, 100 per page.", map[string]property{"cursor": {"type": "string"}}, []string{}), makeTool("storage_read", "Read a UTF-8 text object under this tenant's prefix.", map[string]property{"key": {"type": "string"}}, []string{"key"}))
		if s.cfg.AllowWrite {
			result = append(result, makeTool("storage_write", "Write or replace a UTF-8 text object under this tenant's prefix.", map[string]property{"key": {"type": "string"}, "text": {"type": "string"}}, []string{"key", "text"}), makeTool("storage_delete", "Delete an object under this tenant's prefix.", map[string]property{"key": {"type": "string"}}, []string{"key"}))
		}
	}
	return result
}

func FromEnv() Settings {
	return Settings{Token: os.Getenv("TOOL_API_KEY"), SearchProvider: os.Getenv("SEARCH_PROVIDER"), SearchKey: os.Getenv("SEARCH_API_KEY"), Bucket: os.Getenv("STORAGE_BUCKET"), Region: os.Getenv("STORAGE_REGION"), Endpoint: os.Getenv("STORAGE_ENDPOINT"), Prefix: os.Getenv("STORAGE_PREFIX"), AccessKey: os.Getenv("STORAGE_ACCESS_KEY"), SecretKey: os.Getenv("STORAGE_SECRET_KEY"), AllowWrite: os.Getenv("STORAGE_ALLOW_WRITE") == "true"}
}
