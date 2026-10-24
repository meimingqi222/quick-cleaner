package platform

import "testing"

func TestNVMLLossyName(t *testing.T) {
	for _, tc := range []struct{ input, want string }{
		{"NVIDIA RTX", "NVIDIA RTX"}, {"显卡", "显卡"}, {"\xff\xff", "\ufffd\ufffd"},
		{"\xe2\x82", "\ufffd"}, {"\xe2\x82A", "\ufffdA"}, {"\xed\xa0\x80", "\ufffd\ufffd\ufffd"},
		{"\xf0\x80\x80\x80", "\ufffd\ufffd\ufffd\ufffd"}, {"\xf4\x90\x80\x80", "\ufffd\ufffd\ufffd\ufffd"},
	} {
		if got := nvmlLossyName([]byte(tc.input)); got != tc.want {
			t.Errorf("%x: got %q, want %q", tc.input, got, tc.want)
		}
	}
}
