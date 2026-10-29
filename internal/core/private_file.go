package core

import "os"

func writePrivateFile(path string, data []byte) error {
	f, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY, 0600)
	if err != nil {
		return err
	}
	defer f.Close()
	if err = f.Chmod(0600); err != nil {
		return err
	}
	if err = f.Truncate(0); err != nil {
		return err
	}
	_, err = f.Write(data)
	return err
}

func restrictExistingFile(path string) {
	if f, err := os.Open(path); err == nil {
		_ = f.Chmod(0600)
		_ = f.Close()
	}
}
