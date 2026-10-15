package server

import (
	"crypto/subtle"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"strings"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

var exportSections = []wire.ExportSection{
	wire.ExportSectionHistory, wire.ExportSectionRuns, wire.ExportSectionOperations, wire.ExportSectionInteractions,
	wire.ExportSectionConnections, wire.ExportSectionFiles, wire.ExportSectionState, wire.ExportSectionMemory, wire.ExportSectionSchedules,
}

type exportCursor struct {
	Session    string
	Section    int
	After      string
	Incomplete bool
}

func (s *Server) export(w http.ResponseWriter, r *http.Request, p kernel.Principal) error {
	session, err := s.Control.Session(r.Context(), p.Tenant, r.PathValue("session"))
	if err != nil {
		return err
	}
	cursor := exportCursor{Session: session.View.ID}
	if after := r.URL.Query().Get("after"); after != "" && after != "0" {
		parts := strings.Split(after, ".")
		if len(after) > 4096 || len(parts) != 3 || parts[0] != "e2" ||
			subtle.ConstantTimeCompare([]byte(s.sign("export", parts[1])), []byte(parts[2])) != 1 {
			return kernel.Fail(wire.ErrorCodeInvalid, "invalid export continuation")
		}
		body, e := base64.RawURLEncoding.DecodeString(parts[1])
		if e != nil || json.Unmarshal(body, &cursor) != nil || cursor.Session != session.View.ID ||
			cursor.Section < 0 || cursor.Section >= len(exportSections) || len(cursor.After) > 256 {
			return kernel.Fail(wire.ErrorCodeInvalid, "export continuation does not match this session")
		}
	}
	out := wire.ExportManifest{Format: "brain-export/2", Session: session.View, Section: exportSections[cursor.Section],
		Runs: []wire.Run{}, Operations: []wire.Operation{}, Interactions: []wire.Interaction{}, Connections: []wire.Connection{},
		Files: []wire.FileInfo{}, State: []wire.Document{}, Memory: []wire.Document{}, Schedules: []wire.Schedule{},
		History: wire.RecordPage{Records: []wire.Record{}, Gaps: []wire.Gap{}, Next: "0", AllocatedThrough: session.View.Sequence}}
	next := ""
	switch out.Section {
	case wire.ExportSectionHistory:
		after := cursor.After
		if after == "" {
			after = "0"
		}
		sequence, e := kernel.Sequence(after)
		if e != nil {
			return e
		}
		out.History, err = s.History.Page(r.Context(), session.View.ID, sequence, 128)
		for _, gap := range out.History.Gaps {
			cursor.Incomplete = cursor.Incomplete || gap.Kind != wire.GapKindUnused
		}
		if out.History.More {
			next = out.History.Next
		}
	case wire.ExportSectionRuns:
		var rows []control.Run
		rows, err = s.Control.Runs(r.Context(), session.View.ID, 128, cursor.After)
		for _, row := range rows {
			out.Runs = append(out.Runs, row.View)
			next = row.View.ID
		}
	case wire.ExportSectionOperations:
		var rows []control.Operation
		rows, err = s.Control.Operations(r.Context(), session.View.ID, 128, cursor.After)
		for _, row := range rows {
			out.Operations = append(out.Operations, row.View)
			next = row.View.ID
		}
	case wire.ExportSectionInteractions:
		out.Interactions, err = s.Control.Interactions(r.Context(), session.View.ID, 128, cursor.After)
		if len(out.Interactions) != 0 {
			next = out.Interactions[len(out.Interactions)-1].ID
		}
	case wire.ExportSectionConnections:
		out.Connections, err = s.Control.Connections(r.Context(), session.View.ID, 128, cursor.After)
		if len(out.Connections) != 0 {
			next = out.Connections[len(out.Connections)-1].ID
		}
	case wire.ExportSectionFiles:
		out.Files, err = s.Control.Files(r.Context(), session.View.ID, 128, cursor.After)
		if len(out.Files) != 0 {
			next = out.Files[len(out.Files)-1].Name
		}
	case wire.ExportSectionState:
		out.State, err = s.Control.Documents(r.Context(), session.View.ID, "state", 128, cursor.After)
		if len(out.State) != 0 {
			next = out.State[len(out.State)-1].Key
		}
	case wire.ExportSectionMemory:
		out.Memory, err = s.Control.Documents(r.Context(), session.View.ID, "memory", 128, cursor.After)
		if len(out.Memory) != 0 {
			next = out.Memory[len(out.Memory)-1].Key
		}
	case wire.ExportSectionSchedules:
		out.Schedules, err = s.Control.Schedules(r.Context(), session.View.ID, 128, cursor.After)
		if len(out.Schedules) != 0 {
			next = out.Schedules[len(out.Schedules)-1].ID
		}
	}
	if err != nil {
		return err
	}
	cursor.After = next
	if next == "" {
		cursor.Section++
	}
	if cursor.Section == len(exportSections) {
		out.Complete = !cursor.Incomplete
	} else {
		encoded := base64.RawURLEncoding.EncodeToString(kernel.JSON(cursor))
		out.Continuation = "e2." + encoded + "." + s.sign("export", encoded)
	}
	return writeJSON(w, out)
}
