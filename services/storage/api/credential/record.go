// Package credential defines Storage's opaque private recovery records.
// Storage persists these envelopes without holding decryption keys.
package credential

import "errors"

type Record struct {
	Binding    string `json:"binding"`
	Key        string `json:"key"`
	Ciphertext []byte `json:"ciphertext"`
	Revoked    bool   `json:"revoked,omitempty"`
}

// ValidateCatalog detects missing, duplicate or unexpected recovery records.
func ValidateCatalog(bindings []string, records []Record) error {
	if len(bindings) != len(records) {
		return errors.New("private credential catalog is incomplete")
	}
	seen := map[string]bool{}
	for _, binding := range bindings {
		if seen[binding] {
			return errors.New("duplicate private credential binding")
		}
		seen[binding] = true
	}
	for _, record := range records {
		if !seen[record.Binding] {
			return errors.New("unexpected private credential record")
		}
		delete(seen, record.Binding)
	}
	return nil
}
