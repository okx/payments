package protocol

import "testing"

func TestAmountBinding(t *testing.T) {
	for _, pair := range [][2]string{{"000100", "100"}, {"0", "000"}, {"340282366920938463463374607431768211456", "340282366920938463463374607431768211456"}} {
		if err := CheckAmount(pair[0], pair[1]); err != nil {
			t.Fatal(err)
		}
	}
	for _, value := range []string{"", "-1", "+100", "1.0", "1e2", " 100", "1000"} {
		if err := CheckAmount(value, "100"); err == nil {
			t.Fatalf("accepted %q", value)
		}
	}
}
