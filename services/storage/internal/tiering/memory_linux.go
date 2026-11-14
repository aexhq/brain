package tiering

import (
	"os"
	"strconv"
	"strings"
)

func memoryPressure() (int64, int64) {
	read := func(p string) int64 {
		b, e := os.ReadFile(p)
		if e != nil {
			return 0
		}
		v, _ := strconv.ParseInt(strings.TrimSpace(string(b)), 10, 64)
		return v
	}
	return read("/sys/fs/cgroup/memory.max"), read("/sys/fs/cgroup/memory.current")
}
