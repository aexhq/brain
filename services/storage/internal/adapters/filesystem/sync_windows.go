package filesystem

import "os"

// Files are explicitly flushed before replacement. Windows directory handles do
// not support the portable os.File.Sync operation.
func syncDirectory(*os.Root, string) error { return nil }
