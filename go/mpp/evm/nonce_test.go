package evm

import (
	"testing"

	"github.com/ethereum/go-ethereum/common"
)

func TestUuidNonceProvider_Returns128BitValue(t *testing.T) {
	p := NewUuidNonceProvider()
	nonce, err := p.Allocate(common.Address{}, [32]byte{})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if nonce == nil {
		t.Fatal("expected non-nil nonce")
	}
	if nonce.Sign() <= 0 {
		t.Fatal("expected positive nonce")
	}
	if nonce.BitLen() > 128 {
		t.Fatalf("expected BitLen <= 128, got %d", nonce.BitLen())
	}
}

func TestUuidNonceProvider_UniqueValues(t *testing.T) {
	p := NewUuidNonceProvider()
	seen := make(map[string]struct{}, 100)

	for i := 0; i < 100; i++ {
		nonce, err := p.Allocate(common.Address{}, [32]byte{})
		if err != nil {
			t.Fatalf("unexpected error on iteration %d: %v", i, err)
		}
		key := nonce.String()
		if _, exists := seen[key]; exists {
			t.Fatalf("duplicate nonce on iteration %d: %s", i, key)
		}
		seen[key] = struct{}{}
	}
}
