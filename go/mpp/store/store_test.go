package store

import (
	"context"
	"math/big"
	"sync"
	"testing"
)

// ─────────────────────────────────────────────────────────────────────────────
// MemoryStore[ChannelState]
// ─────────────────────────────────────────────────────────────────────────────

func TestMemoryStore_PutGet(t *testing.T) {
	ctx := context.Background()
	s := NewMemoryStore[ChannelState]()

	cs := &ChannelState{ChannelID: "ch1", Deposit: big.NewInt(1000)}
	if err := s.Put(ctx, "ch1", cs); err != nil {
		t.Fatalf("Put: %v", err)
	}

	got, err := s.Get(ctx, "ch1")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if got == nil {
		t.Fatal("Get returned nil")
	}
	if got.ChannelID != "ch1" {
		t.Errorf("ChannelID: got %q want ch1", got.ChannelID)
	}
	if got.Deposit.Cmp(big.NewInt(1000)) != 0 {
		t.Errorf("Deposit: got %s want 1000", got.Deposit)
	}
}

func TestMemoryStore_GetMissing(t *testing.T) {
	s := NewMemoryStore[ChannelState]()
	got, err := s.Get(context.Background(), "missing")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if got != nil {
		t.Errorf("expected nil for missing key, got %+v", got)
	}
}

func TestMemoryStore_GetReturnsCopy(t *testing.T) {
	ctx := context.Background()
	s := NewMemoryStore[ChannelState]()

	cs := &ChannelState{ChannelID: "ch1", Deposit: big.NewInt(100)}
	s.Put(ctx, "ch1", cs)

	got1, _ := s.Get(ctx, "ch1")
	got1.Deposit = big.NewInt(999)

	got2, _ := s.Get(ctx, "ch1")
	if got2.Deposit.Cmp(big.NewInt(100)) != 0 {
		t.Error("Get did not return a copy — mutation leaked")
	}
}

func TestMemoryStore_Delete(t *testing.T) {
	ctx := context.Background()
	s := NewMemoryStore[ChannelState]()

	s.Put(ctx, "ch1", &ChannelState{ChannelID: "ch1"})

	if err := s.Delete(ctx, "ch1"); err != nil {
		t.Fatalf("Delete: %v", err)
	}

	got, _ := s.Get(ctx, "ch1")
	if got != nil {
		t.Error("expected nil after delete")
	}
}

func TestMemoryStore_DeleteMissing(t *testing.T) {
	s := NewMemoryStore[ChannelState]()
	err := s.Delete(context.Background(), "nope")
	if err != nil {
		t.Errorf("Delete missing: %v", err)
	}
}

func TestMemoryStore_ConcurrentAccess(t *testing.T) {
	ctx := context.Background()
	s := NewMemoryStore[ChannelState]()

	var wg sync.WaitGroup
	for i := range 50 {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			key := "ch1"
			s.Put(ctx, key, &ChannelState{ChannelID: key, Units: uint64(i)})
			s.Get(ctx, key)
		}(i)
	}
	wg.Wait()

	got, _ := s.Get(ctx, "ch1")
	if got == nil {
		t.Fatal("expected non-nil after concurrent writes")
	}
}

// ─────────────────────────────────────────────────────────────────────────────
// DeductFromChannel
// ─────────────────────────────────────────────────────────────────────────────

func TestDeductFromChannel_Success(t *testing.T) {
	ctx := context.Background()
	s := NewMemoryStore[ChannelState]()
	s.Put(ctx, "ch1", &ChannelState{
		ChannelID:            "ch1",
		Deposit:              big.NewInt(1000),
		HighestVoucherAmount: big.NewInt(500),
		Spent:                big.NewInt(0),
	})

	got, err := DeductFromChannel(ctx, s, "ch1", big.NewInt(100))
	if err != nil {
		t.Fatalf("DeductFromChannel: %v", err)
	}
	if got.Spent.Cmp(big.NewInt(100)) != 0 {
		t.Errorf("Spent: got %s want 100", got.Spent)
	}
	if got.Units != 1 {
		t.Errorf("Units: got %d want 1", got.Units)
	}
}

func TestDeductFromChannel_InsufficientBalance(t *testing.T) {
	ctx := context.Background()
	s := NewMemoryStore[ChannelState]()
	s.Put(ctx, "ch1", &ChannelState{
		ChannelID:            "ch1",
		HighestVoucherAmount: big.NewInt(50),
		Spent:                big.NewInt(0),
	})

	_, err := DeductFromChannel(ctx, s, "ch1", big.NewInt(100))
	if err == nil {
		t.Error("expected insufficient balance error")
	}
}

func TestDeductFromChannel_NotFound(t *testing.T) {
	s := NewMemoryStore[ChannelState]()
	_, err := DeductFromChannel(context.Background(), s, "missing", big.NewInt(1))
	if err == nil {
		t.Error("expected not found error")
	}
}

func TestDeductFromChannel_Finalized(t *testing.T) {
	ctx := context.Background()
	s := NewMemoryStore[ChannelState]()
	s.Put(ctx, "ch1", &ChannelState{
		ChannelID:            "ch1",
		HighestVoucherAmount: big.NewInt(1000),
		Spent:                big.NewInt(0),
		Finalized:            true,
	})

	_, err := DeductFromChannel(ctx, s, "ch1", big.NewInt(1))
	if err == nil {
		t.Error("expected finalized error")
	}
}
