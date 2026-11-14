package filesystem

// Files are explicitly flushed before replacement. Windows directory handles do
// not support the portable os.File.Sync operation.
func syncDirectory(string) error { return nil }
