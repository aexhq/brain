package persistence

import (
	"errors"

	"github.com/aws/smithy-go"
	"google.golang.org/api/googleapi"
)

// PermanentError stops background retries until the operator fixes credentials
// or the bucket and explicitly requests synchronization again.
func PermanentError(err error) bool {
	var aws smithy.APIError
	if errors.As(err, &aws) {
		switch aws.ErrorCode() {
		case "AccessDenied", "InvalidAccessKeyId", "SignatureDoesNotMatch", "NoSuchBucket":
			return true
		}
	}
	var gcp *googleapi.Error
	return errors.As(err, &gcp) && (gcp.Code == 401 || gcp.Code == 403 || gcp.Code == 404)
}
