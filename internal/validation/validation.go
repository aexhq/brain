package validation

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"strings"
	"sync"

	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
	"github.com/santhosh-tekuri/jsonschema/v6"
)

type offline struct{}

func (offline) Load(string) (any, error) {
	return nil, fmt.Errorf("remote schema resolution is forbidden")
}

type Validator struct {
	compiler *jsonschema.Compiler
	schemas  map[string]*jsonschema.Schema
	mu       sync.Mutex
}

func New() (*Validator, error) {
	compiler := jsonschema.NewCompiler()
	compiler.UseLoader(offline{})
	doc, err := parse(wire.Schema)
	if err != nil {
		return nil, err
	}
	if err = compiler.AddResource("urn:brain:session:2", doc); err != nil {
		return nil, err
	}
	return &Validator{compiler: compiler, schemas: map[string]*jsonschema.Schema{}}, nil
}

func parse(body []byte) (any, error) {
	decoder := json.NewDecoder(bytes.NewReader(body))
	decoder.UseNumber()
	value, err := readValue(decoder, 0, new(int))
	if err != nil {
		return nil, err
	}
	if _, err := decoder.Token(); err != io.EOF {
		return nil, fmt.Errorf("one JSON value is required")
	}
	return value, nil
}

func readValue(decoder *json.Decoder, depth int, nodes *int) (any, error) {
	*nodes++
	if depth > 32 || *nodes > 32768 {
		return nil, fmt.Errorf("JSON depth or node limit exceeded")
	}
	token, err := decoder.Token()
	if err != nil {
		return nil, err
	}
	switch token {
	case json.Delim('{'):
		out := map[string]any{}
		for decoder.More() {
			key, err := decoder.Token()
			if err != nil {
				return nil, err
			}
			name, ok := key.(string)
			if !ok {
				return nil, fmt.Errorf("invalid object key")
			}
			if _, exists := out[name]; exists {
				return nil, fmt.Errorf("duplicate JSON key")
			}
			value, err := readValue(decoder, depth+1, nodes)
			if err != nil {
				return nil, err
			}
			out[name] = value
		}
		_, err := decoder.Token()
		return out, err
	case json.Delim('['):
		out := []any{}
		for decoder.More() {
			value, err := readValue(decoder, depth+1, nodes)
			if err != nil {
				return nil, err
			}
			out = append(out, value)
		}
		_, err := decoder.Token()
		return out, err
	default:
		return token, nil
	}
}

func (v *Validator) Wire(name string, body []byte) error {
	value, err := parse(body)
	if err != nil {
		return kernel.Fail(wire.ErrorCodeInvalid, "invalid JSON: "+err.Error())
	}
	v.mu.Lock()
	schema := v.schemas[name]
	if schema == nil {
		schema, err = v.compiler.Compile("urn:brain:session:2#/$defs/" + name)
		if err == nil {
			v.schemas[name] = schema
		}
	}
	v.mu.Unlock()
	if err != nil {
		return err
	}
	if err = schema.Validate(value); err != nil {
		return kernel.Fail(wire.ErrorCodeInvalid, "request does not match "+name)
	}
	return nil
}

func Schema(schema []byte) (*jsonschema.Schema, error) {
	if len(schema) > 16384 {
		return nil, kernel.Fail(wire.ErrorCodeInvalid, "schema is larger than 16 KiB")
	}
	value, err := parse(schema)
	if err != nil {
		return nil, kernel.Fail(wire.ErrorCodeInvalid, "invalid bounded schema")
	}
	if err = checkSchema(value, 0); err != nil {
		return nil, err
	}
	compiler := jsonschema.NewCompiler()
	compiler.UseLoader(offline{})
	if err = compiler.AddResource("urn:brain:application", value); err != nil {
		return nil, err
	}
	compiled, err := compiler.Compile("urn:brain:application")
	if err != nil {
		return nil, kernel.Fail(wire.ErrorCodeInvalid, "application schema cannot be compiled without remote resources")
	}
	return compiled, nil
}

func checkSchema(value any, depth int) error {
	if depth > 16 {
		return kernel.Fail(wire.ErrorCodeInvalid, "schema nesting exceeds 16")
	}
	switch node := value.(type) {
	case map[string]any:
		for key, child := range node {
			if key == "$ref" || key == "$dynamicRef" {
				ref, ok := child.(string)
				if !ok || !strings.HasPrefix(ref, "#") {
					return kernel.Fail(wire.ErrorCodeInvalid, "remote schema references are forbidden")
				}
			}
			if key == "$id" {
				return kernel.Fail(wire.ErrorCodeInvalid, "application schemas cannot change resolution scope")
			}
			if err := checkSchema(child, depth+1); err != nil {
				return err
			}
		}
	case []any:
		for _, child := range node {
			if err := checkSchema(child, depth+1); err != nil {
				return err
			}
		}
	}
	return nil
}

func Validate(schema, body []byte) error {
	compiled, err := Schema(schema)
	if err != nil {
		return err
	}
	value, err := parse(body)
	if err != nil {
		return kernel.Fail(wire.ErrorCodeInvalid, "invalid application payload")
	}
	if err = compiled.Validate(value); err != nil {
		return kernel.Fail(wire.ErrorCodeInvalid, "application payload does not match its admitted schema")
	}
	return nil
}
