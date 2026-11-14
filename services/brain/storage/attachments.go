package storage

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	core "github.com/aexhq/brain/services/brain/api"
	api "github.com/aexhq/brain/services/storage/api"
	"io"
	"net/http"
	"net/url"
	"slices"
	"strings"
	"time"
	"unicode/utf8"
)

const AttachmentBytes = 16 << 20
const modelInputBytes = 1 << 20

func (w *Workspace) attachments(ctx context.Context, i api.Identity, ref string) ([]core.Attachment, error) {
	if ref == "" {
		return []core.Attachment{}, nil
	}
	o, err := w.Client.GetObject(ctx, i, ref)
	if err != nil {
		return nil, err
	}
	var out []core.Attachment
	err = json.Unmarshal(o.Data, &out)
	return out, err
}

// UploadAttachment links a completed stream to the session with one metadata
// write. Partial uploads are never message-addressable objects.
func (w *Workspace) UploadAttachment(ctx context.Context, id, name, mediaType string, body io.Reader) (core.Attachment, error) {
	var out core.Attachment
	if name == "" || len(name) > 255 || strings.ContainsAny(name, "\r\n\x00") || len(mediaType) > 128 || !strings.Contains(mediaType, "/") {
		return out, core.E(core.Invalid, "invalid attachment descriptor")
	}
	i := w.identity(ctx, id)
	m := w.lock(i)
	m.Lock()
	defer m.Unlock()
	r, err := w.record(ctx, i)
	if err != nil {
		return out, err
	}
	if r.Deleted {
		return out, core.E(core.NotFound, "session was deleted")
	}
	items, err := w.attachments(ctx, i, r.Attachments)
	if err != nil {
		return out, err
	}
	if len(items) >= 128 {
		return out, core.E(core.Overloaded, "session attachment limit reached")
	}
	ref, size, err := w.Client.Upload(ctx, i, body, AttachmentBytes)
	if err != nil {
		return out, err
	}
	out = core.Attachment{Reference: ref, Name: name, MediaType: mediaType, Size: size}
	items = append(items, out)
	data, err := json.Marshal(items)
	if err != nil {
		return out, err
	}
	refs := make([]string, 0, len(items))
	for _, v := range items {
		refs = append(refs, v.Reference)
	}
	r.Attachments, err = w.Client.PutObject(ctx, i, api.Object{Data: data, References: refs})
	if err != nil {
		return out, err
	}
	return out, w.put(ctx, i, r, r.Revision)
}

func (w *Workspace) Attachment(ctx context.Context, id, reference string) (core.Attachment, error) {
	var empty core.Attachment
	if !core.ValidObjectReference(reference) {
		return empty, core.E(core.Invalid, "invalid attachment reference")
	}
	i := w.identity(ctx, id)
	r, err := w.record(ctx, i)
	if err != nil {
		return empty, err
	}
	if r.Deleted {
		return empty, core.E(core.NotFound, "session was deleted")
	}
	items, err := w.attachments(ctx, i, r.Attachments)
	if err != nil {
		return empty, err
	}
	for _, item := range items {
		if item.Reference == reference {
			return item, nil
		}
	}
	return empty, core.E(core.NotFound, "attachment is not finalized in this session")
}
func (w *Workspace) DownloadAttachment(ctx context.Context, id string, item core.Attachment, dest io.Writer) error {
	_, err := w.Client.Download(ctx, w.identity(ctx, id), item.Reference, dest, item.Size)
	return err
}
func supportedInput(mime string) bool {
	return strings.HasPrefix(mime, "text/") || mime == "application/json" || slices.Contains([]string{"image/png", "image/jpeg", "image/webp", "image/gif"}, mime)
}
func (w *Workspace) ValidateInput(ctx context.Context, id string, input core.Input) error {
	if len(input.Attachments)+len(input.References) > 16 {
		return core.E(core.Invalid, "at most 16 input references are permitted")
	}
	var size int64
	for _, ref := range input.Attachments {
		item, err := w.Attachment(ctx, id, ref)
		if err != nil {
			return err
		}
		size += item.Size
		if !supportedInput(item.MediaType) || size > modelInputBytes {
			return core.E(core.Invalid, "model attachments require text or images within 1 MiB total")
		}
	}
	for _, ref := range input.References {
		if _, err := w.referenceRequest(ctx, ref); err != nil {
			return err
		}
	}
	return nil
}

func (w *Workspace) referenceRequest(ctx context.Context, ref core.Reference) (*http.Request, error) {
	u, err := url.Parse(ref.URI)
	if err != nil || len(ref.URI) > 4096 || u.User != nil || u.Fragment != "" || !supportedInput(ref.MediaType) {
		return nil, core.E(core.Invalid, "invalid external reference")
	}
	for _, origin := range w.MediaOrigins {
		if origin != u.Scheme+"://"+u.Host {
			continue
		}
		// The deployment chooses the destination. User input supplies only the
		// path/query, never the host, scheme, credentials or redirect policy.
		req, err := http.NewRequestWithContext(ctx, http.MethodGet, origin, nil)
		if err != nil {
			return nil, err
		}
		req.URL.Path, req.URL.RawPath, req.URL.RawQuery = u.Path, u.RawPath, u.RawQuery
		return req, nil
	}
	return nil, core.E(core.Forbidden, "reference origin is not authorized")
}
func (w *Workspace) ResolveInput(ctx context.Context, id string, input core.Input) (core.Text, error) {
	out := core.Text{Text: input.Text, Media: input.Media}
	if err := w.ValidateInput(ctx, id, input); err != nil {
		return out, err
	}
	remaining := int64(modelInputBytes)
	appendData := func(name, mime string, data []byte) error {
		remaining -= int64(len(data))
		if remaining < 0 {
			return core.E(core.Invalid, "resolved inputs exceed 1 MiB")
		}
		if strings.HasPrefix(mime, "image/") {
			out.Media = append(out.Media, core.Media{Type: "image", MimeType: mime, Data: base64.StdEncoding.EncodeToString(data)})
		} else {
			if !utf8.Valid(data) {
				return core.E(core.Invalid, "text attachment must be UTF-8")
			}
			out.Text += "\n\n" + name + ":\n" + string(data)
		}
		return nil
	}
	for _, ref := range input.Attachments {
		item, err := w.Attachment(ctx, id, ref)
		if err != nil {
			return out, err
		}
		var data bytes.Buffer
		if err = w.DownloadAttachment(ctx, id, item, &data); err != nil {
			return out, err
		}
		if err = appendData(item.Name, item.MediaType, data.Bytes()); err != nil {
			return out, err
		}
	}
	client := &http.Client{Timeout: 15 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
	for _, ref := range input.References {
		req, err := w.referenceRequest(ctx, ref)
		if err != nil {
			return out, err
		}
		res, err := client.Do(req)
		if err != nil {
			return out, err
		}
		if res.StatusCode != 200 {
			res.Body.Close()
			return out, core.E(core.Invalid, "external reference is unavailable")
		}
		data, err := io.ReadAll(io.LimitReader(res.Body, remaining+1))
		res.Body.Close()
		if err != nil {
			return out, err
		}
		if err = appendData(ref.URI, ref.MediaType, data); err != nil {
			return out, err
		}
	}
	return out, nil
}
