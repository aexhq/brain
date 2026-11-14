package api

import (
	"context"
	"errors"
	core "github.com/aexhq/brain/services/brain/api"
	"io"
	"mime"
	"net/http"
	"strconv"
)

type Attachments interface {
	UploadAttachment(context.Context, string, string, string, io.Reader) (core.Attachment, error)
	Attachment(context.Context, string, string) (core.Attachment, error)
	DownloadAttachment(context.Context, string, core.Attachment, io.Writer) error
}

func (s *Server) uploadAttachment(w http.ResponseWriter, r *http.Request, g core.Grant) {
	if s.Attachments == nil {
		s.fail(w, r, core.E(core.Unsupported, "attachments require distributed storage"))
		return
	}
	if _, err := s.Engine.Metadata(r.Context(), g, r.PathValue("id")); err != nil {
		s.fail(w, r, err)
		return
	}
	mediaType, _, err := mime.ParseMediaType(r.Header.Get("Content-Type"))
	if err != nil {
		s.fail(w, r, core.E(core.Invalid, "a valid Content-Type is required"))
		return
	}
	ctx := core.WithTenant(r.Context(), g.Tenant)
	item, err := s.Attachments.UploadAttachment(ctx, r.PathValue("id"), r.URL.Query().Get("name"), mediaType, http.MaxBytesReader(w, r.Body, 16<<20))
	if err != nil {
		var limit *http.MaxBytesError
		if errors.As(err, &limit) {
			write(w, http.StatusRequestEntityTooLarge, ErrorResponse{Error: core.Error{Code: core.Invalid, Message: "attachment exceeds 16 MiB"}, RequestID: w.Header().Get("X-Request-ID")})
			return
		}
		s.fail(w, r, err)
		return
	}
	write(w, 201, item)
}
func (s *Server) downloadAttachment(w http.ResponseWriter, r *http.Request, g core.Grant) {
	if s.Attachments == nil {
		s.fail(w, r, core.E(core.Unsupported, "attachments require distributed storage"))
		return
	}
	if _, err := s.Engine.Metadata(r.Context(), g, r.PathValue("id")); err != nil {
		s.fail(w, r, err)
		return
	}
	ctx := core.WithTenant(r.Context(), g.Tenant)
	item, err := s.Attachments.Attachment(ctx, r.PathValue("id"), r.URL.Query().Get("reference"))
	if err != nil {
		s.fail(w, r, err)
		return
	}
	w.Header().Set("Content-Type", item.MediaType)
	w.Header().Set("Content-Length", strconv.FormatInt(item.Size, 10))
	w.Header().Set("Content-Disposition", mime.FormatMediaType("attachment", map[string]string{"filename": item.Name}))
	if err := s.Attachments.DownloadAttachment(ctx, r.PathValue("id"), item, w); err != nil {
		s.Logger.Error("attachment stream failed", "error", err)
		panic(http.ErrAbortHandler)
	}
}
