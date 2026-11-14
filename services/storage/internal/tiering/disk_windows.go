package tiering

import "golang.org/x/sys/windows"

func diskFree(path string) (int64, error) {
	name, err := windows.UTF16PtrFromString(path)
	if err != nil {
		return 0, err
	}
	var free uint64
	err = windows.GetDiskFreeSpaceEx(name, &free, nil, nil)
	return int64(free), err
}
