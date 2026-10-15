package control

import "context"

type Authority struct {
	Participant string
	Session     string
	Tenant      string
	Subject     string   `json:"Subject,omitempty"`
	Permissions []string `json:"Permissions,omitempty"`
	Expires     int64    `json:"Expires,omitempty"`
}

type authorityKey struct{}

func WithAuthority(ctx context.Context, authority Authority) context.Context {
	return context.WithValue(ctx, authorityKey{}, authority)
}

func RequestAuthority(ctx context.Context) *Authority {
	value, ok := ctx.Value(authorityKey{}).(Authority)
	if !ok {
		return nil
	}
	return &value
}
