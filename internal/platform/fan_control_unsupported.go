//go:build !darwin

package platform

func FanControlSupported() bool { return false }

func SetFanMode(FanMode) error {
	return FanError{Kind: FanOther, Detail: "当前平台不支持风扇控制"}
}
