package main

import (
	"encoding/json"
	"reflect"
	"testing"

	core "github.com/aexhq/brain/services/brain/api"
)

func TestPublicSchemaExcludesPrivateRecoveryState(t *testing.T) {
	r := registry()
	r.Schema(reflect.TypeFor[core.Session](), true, "")
	data, err := json.Marshal(r)
	if err != nil {
		t.Fatal(err)
	}
	var schemas map[string]struct {
		Properties map[string]json.RawMessage `json:"properties"`
	}
	if err := json.Unmarshal(data, &schemas); err != nil {
		t.Fatal(err)
	}
	for name, fields := range map[string][]string{"Session": {"tenant", "credentials"}, "Checkpoint": {"files", "config"}} {
		for _, field := range fields {
			if _, ok := schemas[name].Properties[field]; ok {
				t.Fatalf("private %s.%s exposed", name, field)
			}
		}
		if len(schemas[name].Properties["id"]) == 0 {
			t.Fatalf("missing public %s.id", name)
		}
	}
	if _, ok := schemas["CredentialRecord"]; ok {
		t.Fatal("private credential records exposed")
	}
}
