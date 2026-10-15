package sqlcontrol

import (
	"context"
	"database/sql"
	"encoding/json"
	"regexp"

	"github.com/aexhq/brain/internal/control"
	"github.com/aexhq/brain/internal/generated/wire"
	"github.com/aexhq/brain/internal/kernel"
)

var immutableImage = regexp.MustCompile(`^(?:[A-Za-z0-9._:/-]+@)?sha256:[a-f0-9]{64}$`)

func (s *Store) settlePackage(ctx context.Context, tx *sql.Tx, session control.Session, operation control.Operation, receipt wire.Receipt) error {
	if operation.View.Action != "build_package" {
		return nil
	}
	action, err := kernel.Action(session.View.Config, operation.View.Action)
	if err != nil {
		return err
	}
	var provider string
	for _, template := range session.View.Config.Connections {
		if template.Name == action.Connection {
			provider = template.Provider
		}
	}
	if provider != "local_compute" && provider != "modal" && provider != "fly" {
		return nil
	}
	var arguments struct {
		PackageID string `json:"package_id"`
	}
	if json.Unmarshal(operation.Arguments, &arguments) != nil || arguments.PackageID == "" {
		return nil
	}
	var body string
	query := "SELECT body FROM packages WHERE tenant=? AND id=?"
	if s.postgres {
		query += " FOR UPDATE"
	}
	if err = tx.QueryRowContext(ctx, s.q(query), session.Tenant, arguments.PackageID).Scan(&body); err != nil {
		return err
	}
	pkg, err := decode[wire.Package](body)
	if err != nil || pkg.Status == "ready" {
		return err
	}
	pkg.Status, pkg.Error = string(receipt.Outcome), receipt.Error
	if receipt.Outcome == wire.OutcomeCompleted {
		var result wire.PackageBuildResult
		if json.Unmarshal(receipt.Result, &result) != nil || result.PackageID != pkg.ID ||
			result.SourceDigest != pkg.Digest || result.Builder != provider || !immutableImage.MatchString(result.Image) ||
			!immutableImage.MatchString(result.BaseImage) {
			pkg.Status, pkg.Error = "failed", kernel.Ptr("completed build has invalid immutable image provenance")
		} else {
			pkg.Status, pkg.Image, pkg.Error = "ready", &result.Image, nil
		}
	}
	_, err = tx.ExecContext(ctx, s.q("UPDATE packages SET body=? WHERE tenant=? AND id=?"), string(kernel.JSON(pkg)), session.Tenant, pkg.ID)
	return err
}
