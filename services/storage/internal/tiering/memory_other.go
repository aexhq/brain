//go:build !linux

package tiering

func memoryPressure() (int64, int64) { return 0, 0 }
