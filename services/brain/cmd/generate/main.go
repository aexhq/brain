// Command generate derives OpenAPI schemas from the Go wire types and routes.
package main

import (
	"bytes"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"strconv"
	"strings"

	"github.com/aexhq/brain/runtimes/host"
	core "github.com/aexhq/brain/services/brain/api"
	"github.com/aexhq/brain/services/brain/internal/http"
	"github.com/danielgtaylor/huma/v2"
)

func registry() huma.Registry {
	return huma.NewMapRegistry("#/components/schemas/", func(t reflect.Type, hint string) string {
		for t.Kind() == reflect.Pointer {
			t = t.Elem()
		}
		switch t {
		case reflect.TypeFor[core.Config]():
			return "SessionConfig"
		case reflect.TypeFor[core.Error]():
			return "ErrorDetail"
		case reflect.TypeFor[harness.Options]():
			return "OpenOptions"
		}
		return huma.DefaultSchemaNamer(t, hint)
	})
}

func main() {
	check := flag.Bool("check", false, "fail if generated schemas differ")
	flag.Parse()
	schemas := registry()
	for _, t := range []reflect.Type{reflect.TypeFor[core.ModelRequest](), reflect.TypeFor[core.Call](), reflect.TypeFor[core.Result](), reflect.TypeFor[core.ToolResult]()} {
		schemas.Schema(t, true, "")
	}
	paths := map[string]any{}
	for _, route := range api.Contract() {
		output := schemas.Schema(route.Output, true, "")
		var outputSchema any = output
		if route.Name == "send" {
			status := schemas.Schema(reflect.TypeFor[core.Status](), true, "")
			outputSchema = map[string]any{"oneOf": []any{output, status}}
		}
		content := map[string]any{"application/json": map[string]any{"schema": outputSchema}}
		if route.Name == "downloadAttachment" {
			content = map[string]any{"application/octet-stream": map[string]any{"schema": map[string]any{"type": "string", "format": "binary"}}}
		}
		if route.Name == "events" {
			content["text/event-stream"] = map[string]any{"schema": map[string]any{"type": "string"}, "example": "id: OPAQUE_CURSOR\nevent: assistant.message\ndata: {\"id\":\"EVENT_ID\",\"type\":\"assistant.message\",\"time\":\"2026-10-15T12:00:00Z\",\"input_id\":\"INPUT_ID\",\"data\":{\"text\":\"Hello\"},\"cursor\":\"OPAQUE_CURSOR\"}\n\n"}
		}
		if route.Name == "executorCommands" {
			content = map[string]any{"text/event-stream": map[string]any{"schema": map[string]any{"type": "string"}, "example": "data: {\"type\":\"ready\",\"connection\":\"CONNECTION_ID\"}\n\n"}}
		}
		if route.Name == "executorRPC" {
			content = map[string]any{}
		}
		errorSchema := schemas.Schema(reflect.TypeFor[api.ErrorResponse](), true, "")
		responses := map[string]any{
			strconv.Itoa(route.Status): map[string]any{"description": route.Summary, "content": content},
			"default":                  map[string]any{"description": "Error. 400 invalid_request; 401 missing/invalid Bearer token; 403 forbidden; 404 not_found; 409 conflict or cursor_unavailable; 413 body limit; 422 unsupported; 429 overloaded (Retry-After: 1); 500 brain_error. X-Request-ID is present on every response.", "content": map[string]any{"application/json": map[string]any{"schema": errorSchema}}},
		}
		op := map[string]any{"operationId": route.Name, "summary": route.Summary, "description": route.Description, "security": []any{map[string]any{"bearer": []string{}}}, "responses": responses}
		parameters := []any{}
		if route.Name == "uploadAttachment" || route.Name == "downloadAttachment" {
			name := "reference"
			if route.Name == "uploadAttachment" {
				name = "name"
				op["requestBody"] = map[string]any{"required": true, "content": map[string]any{"application/octet-stream": map[string]any{"schema": map[string]any{"type": "string", "format": "binary"}}}}
			}
			parameters = append(parameters, map[string]any{"name": name, "in": "query", "required": true, "schema": map[string]any{"type": "string"}})
		}
		if strings.Contains(route.Path, "{id}") {
			description := "Opaque session ID."
			schema := map[string]any{"type": "string", "minLength": 1, "maxLength": 128}
			if strings.HasPrefix(route.Path, "/executors/") {
				description = "Opaque executor ID."
				schema = map[string]any{"type": "string", "pattern": "^[a-fA-F0-9]{32}$"}
			}
			parameters = append(parameters, map[string]any{"name": "id", "in": "path", "description": description, "required": true, "schema": schema})
		}
		if route.Name == "list" || route.Name == "events" {
			afterDescription := "Last session ID from the previous page."
			if route.Name == "events" {
				afterDescription = "Opaque cursor from the last retained event."
			}
			parameters = append(parameters,
				map[string]any{"name": "after", "in": "query", "description": afterDescription, "schema": map[string]any{"type": "string"}},
				map[string]any{"name": "limit", "in": "query", "description": "Maximum number of items to return (JSON mode).", "schema": map[string]any{"type": "integer", "default": 100, "minimum": 1, "maximum": 1000}},
			)
		}
		if route.Name == "events" {
			parameters = append(parameters, map[string]any{"name": "tail", "in": "query", "description": "Read the most recent limit events; cannot be combined with after.", "schema": map[string]any{"type": "boolean", "default": false}})
			parameters = append(parameters, map[string]any{"name": "Last-Event-ID", "in": "header", "description": "SSE reconnect cursor (used when the after query parameter is omitted).", "schema": map[string]any{"type": "string"}})
		}
		if len(parameters) != 0 {
			op["parameters"] = parameters
		}
		if route.Input != nil {
			schema := schemas.Schema(route.Input, true, "")
			op["requestBody"] = map[string]any{"required": true, "content": map[string]any{"application/json": map[string]any{"schema": schema}}}
		}
		if paths[route.Path] == nil {
			paths[route.Path] = map[string]any{}
		}
		paths[route.Path].(map[string]any)[strings.ToLower(route.Method)] = op
	}
	apiDoc := map[string]any{"openapi": "3.1.0", "info": map[string]string{"title": "Brain", "version": "current", "description": "HTTP API for the Brain agent session server."}, "servers": []any{map[string]string{"url": "http://127.0.0.1:8080", "description": "Local development"}}, "paths": paths, "components": map[string]any{"schemas": schemas, "securitySchemes": map[string]any{"bearer": map[string]string{"type": "http", "scheme": "bearer"}}}}

	runtimeSchemas := registry()
	for _, t := range []reflect.Type{reflect.TypeFor[harness.Options](), reflect.TypeFor[harness.RunnerFrame](), reflect.TypeFor[core.NativeFile](), reflect.TypeFor[core.ModelRequest](), reflect.TypeFor[core.Result]()} {
		runtimeSchemas.Schema(t, true, "")
	}
	runtimeDoc := map[string]any{"openapi": "3.1.0", "info": &huma.Info{Title: "Brain agent protocol", Version: "current"}, "paths": map[string]any{}, "components": map[string]any{"schemas": runtimeSchemas}}
	files := map[string]any{"services/brain/api/openapi.json": apiDoc, "runtimes/host/protocol.json": runtimeDoc}
	failed := false
	for file, value := range files {
		data, err := json.MarshalIndent(value, "", "  ")
		if err != nil {
			panic(err)
		}
		data = append(data, '\n')
		if *check {
			old, err := os.ReadFile(file)
			if err != nil || !bytes.Equal(old, data) {
				fmt.Fprintln(os.Stderr, "generated file differs:", file)
				failed = true
			}
		} else {
			if err := os.MkdirAll(filepath.Dir(file), 0755); err != nil {
				panic(err)
			}
			if err := os.WriteFile(file, data, 0644); err != nil {
				panic(err)
			}
		}
	}
	if failed {
		os.Exit(1)
	}
}
