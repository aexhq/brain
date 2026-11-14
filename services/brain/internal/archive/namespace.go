package persistence

import (
	"context"
	"errors"
	core "github.com/aexhq/brain/services/brain/api"
	"os"
)

type scopedObjects struct {
	Objects
	prefix string
}

func tenantPrefix(tenant string) string { return "tenants/" + core.SessionDirectory(tenant) + "/" }
func sessionPrefix(id string) string    { return "sessions/" + core.SessionDirectory(id) }
func (s scopedObjects) Get(ctx context.Context, key string) ([]byte, error) {
	return s.Objects.Get(ctx, s.prefix+key)
}
func (s scopedObjects) Put(ctx context.Context, key string, data []byte, immutable bool) error {
	return s.Objects.Put(ctx, s.prefix+key, data, immutable)
}
func (s scopedObjects) DeletePrefix(ctx context.Context, prefix string) error {
	return s.Objects.DeletePrefix(ctx, s.prefix+prefix)
}
func (a *Archive) scoped(tenant string) *Archive {
	copy := *a
	copy.Objects = scopedObjects{Objects: a.Objects, prefix: tenantPrefix(tenant)}
	return &copy
}
func (a *Archive) Publish(ctx context.Context, s Snapshot) (Manifest, error) {
	return a.scoped(s.Session.Tenant).publish(ctx, s)
}
func (a *Archive) Restore(ctx context.Context, tenant, id, reference, parent string) (Manifest, string, error) {
	m, dir, err := a.scoped(tenant).restore(ctx, tenant, id, reference, parent)
	if errors.Is(err, os.ErrNotExist) {
		err = core.E(core.NotFound, "checkpoint not found")
	}
	return m, dir, err
}
func (a *Archive) Delete(ctx context.Context, tenant, id string) error {
	return a.scoped(tenant).delete(ctx, tenant, id)
}
