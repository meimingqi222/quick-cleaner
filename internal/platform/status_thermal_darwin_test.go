package platform

import "testing"

func TestSMCReadProtocol(t *testing.T) {
	for _, fail := range []string{"", "info-kernel", "info-result", "info-short", "size-zero", "size-large", "bytes-kernel", "bytes-result", "bytes-short"} {
		t.Run(fail, func(t *testing.T) {
			calls := 0
			api := smcAPI{call: func(conn, selector uint32, in *smcKeyData, inSize uintptr, out *smcKeyData, outSize *uintptr) int32 {
				calls++
				if conn != 123 || selector != 2 || in.Key != smcKeyCode("F0Ac") || inSize != 80 || *outSize != 80 {
					t.Fatal("wrong call ABI")
				}
				stage := "info"
				if calls == 1 {
					if in.Command != 9 {
						t.Fatal("expected READ_KEYINFO")
					}
					out.Info = smcKeyInfo{Size: 4, Type: smcKeyCode("flt "), Attributes: 255}
					if fail == "size-zero" {
						out.Info.Size = 0
					}
					if fail == "size-large" {
						out.Info.Size = 33
					}
				} else {
					stage = "bytes"
					if calls != 2 || in.Command != 5 || in.Info.Size != 4 || in.Info.Type != smcKeyCode("flt ") || in.Info.Attributes != 0 {
						t.Fatal("expected READ_BYTES with key metadata only")
					}
					out.Bytes = smcTestFloat(1500.25).Bytes
				}
				if fail == stage+"-kernel" {
					return -1
				}
				if fail == stage+"-result" {
					out.Result = 1
				}
				if fail == stage+"-short" {
					*outSize = 79
				}
				return 0
			}}
			v, ok := api.read(123, "F0Ac")
			if ok != (fail == "") {
				t.Fatalf("success=%v for %s", ok, fail)
			}
			if ok {
				if rpm, ok := v.scalar(); !ok || rpm != 1500.25 {
					t.Fatalf("value %+v", v)
				}
			}
		})
	}
}

func TestSMCThermalConnectionLifetime(t *testing.T) {
	for _, fail := range []string{"", "match", "service", "open", "zero-connection"} {
		t.Run(fail, func(t *testing.T) {
			released, closed, reads := 0, 0, 0
			api := smcAPI{
				matching: func(name string) uintptr {
					if name != "AppleSMC" {
						t.Fatal(name)
					}
					if fail == "match" {
						return 0
					}
					return 1
				},
				service: func(port uint32, matcher uintptr) uint32 {
					if port != 0 || matcher != 1 {
						t.Fatal("matcher")
					}
					if fail == "service" {
						return 0
					}
					return 2
				},
				taskSelf: func() uint32 { return 3 },
				open: func(device, task, kind uint32, conn *uint32) int32 {
					if device != 2 || task != 3 || kind != 0 {
						t.Fatal("open")
					}
					if fail == "open" {
						return -1
					}
					if fail != "zero-connection" {
						*conn = 4
					}
					return 0
				},
				release: func(device uint32) int32 {
					if device != 2 {
						t.Fatal("release")
					}
					released++
					return 0
				},
				close: func(conn uint32) int32 {
					if conn != 4 {
						t.Fatal("close")
					}
					closed++
					return 0
				},
				call: func(conn, selector uint32, in *smcKeyData, size uintptr, out *smcKeyData, outSize *uintptr) int32 {
					reads++
					return -1
				},
			}
			got := api.readThermal()
			if got.CPUTemp != nil || len(got.Fans) != 0 {
				t.Fatal("failed sensors must remain unknown")
			}
			wantRelease, wantClose, wantReads := 1, 0, 0
			if fail == "match" || fail == "service" {
				wantRelease = 0
			}
			if fail == "" {
				wantClose, wantReads = 1, 6
			}
			if released != wantRelease || closed != wantClose || reads != wantReads {
				t.Fatalf("release=%d close=%d reads=%d", released, closed, reads)
			}
		})
	}
}
