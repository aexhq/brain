package credentialcipher

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"github.com/aexhq/brain/services/storage/api"
	"github.com/aexhq/brain/services/storage/api/credential"
	"io"
)

func SealRecords(ctx context.Context, keys Keys, tenant, session string, values []credential.Record) ([]Record, error) {
	var records []Record
	for _, v := range values {
		if v.Revoked {
			records = append(records, Record{Binding: v.Binding, Revoked: true})
			continue
		}
		if !json.Valid(v.Value) {
			return nil, errors.New("invalid private credential value")
		}
		r, err := Seal(ctx, keys, Scope{tenant, session, v.Binding}, v.Value)
		if err != nil {
			return nil, err
		}
		records = append(records, r)
	}
	return records, nil
}
func OpenRecords(ctx context.Context, keys Keys, tenant, session string, records []Record) ([]credential.Record, error) {
	var values []credential.Record
	for _, r := range records {
		v := credential.Record{Binding: r.Binding, Revoked: r.Revoked}
		if !r.Revoked {
			b, err := Open(ctx, keys, Scope{tenant, session, r.Binding}, r)
			if err != nil {
				return nil, err
			}
			if !json.Valid(b) {
				return nil, errors.New("invalid private credential value")
			}
			v.Value = b
		}
		values = append(values, v)
	}
	return values, nil
}

type storedRecord struct {
	api.Record
	Credentials []Record `json:"credentials,omitempty"`
}

func EncodeRecord(ctx context.Context, keys Keys, r api.Record) ([]byte, error) {
	records, err := SealRecords(ctx, keys, r.Tenant, r.Session.ID, r.Credentials)
	if err != nil {
		return nil, err
	}
	return json.Marshal(storedRecord{Record: r, Credentials: records})
}
func DecodeRecord(ctx context.Context, keys Keys, data []byte) (api.Record, error) {
	var r storedRecord
	if err := decode(data, &r); err != nil {
		return api.Record{}, err
	}
	values, err := OpenRecords(ctx, keys, r.Tenant, r.Session.ID, r.Credentials)
	if err != nil {
		return api.Record{}, err
	}
	r.Record.Session.Tenant = r.Tenant
	r.Record.Credentials = values
	r.Record.Session.Credentials = values
	if err := credential.ValidateCatalog(r.Session.CredentialBindings, values); err != nil {
		return api.Record{}, err
	}
	return r.Record, nil
}

type storedManifest struct {
	api.Manifest
	Credentials []Record `json:"credentials,omitempty"`
}

func EncodeManifest(ctx context.Context, keys Keys, m api.Manifest) ([]byte, error) {
	records, err := SealRecords(ctx, keys, m.Tenant, m.Session.ID, m.Credentials)
	if err != nil {
		return nil, err
	}
	return json.Marshal(storedManifest{Manifest: m, Credentials: records})
}
func DecodeManifest(ctx context.Context, keys Keys, data []byte) (api.Manifest, error) {
	var m storedManifest
	if err := decode(data, &m); err != nil {
		return api.Manifest{}, err
	}
	values, err := OpenRecords(ctx, keys, m.Tenant, m.Session.ID, m.Credentials)
	if err != nil {
		return api.Manifest{}, err
	}
	m.Manifest.Session.Tenant = m.Tenant
	m.Manifest.Credentials = values
	m.Manifest.Session.Credentials = values
	if err := credential.ValidateCatalog(m.Session.CredentialBindings, values); err != nil {
		return api.Manifest{}, err
	}
	return m.Manifest, nil
}
func decode(data []byte, v any) error {
	d := json.NewDecoder(bytes.NewReader(data))
	d.DisallowUnknownFields()
	if err := d.Decode(v); err != nil {
		return err
	}
	if d.Decode(new(any)) != io.EOF {
		return errors.New("trailing stored JSON")
	}
	return nil
}
