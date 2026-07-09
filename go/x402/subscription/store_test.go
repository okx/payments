package subscription

import (
	"context"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func u64(v uint64) *uint64 { return &v }
func u32(v uint32) *uint32 { return &v }

func TestInMemoryStoreRoundTrip(t *testing.T) {
	ctx := context.Background()
	store := NewInMemoryStore()

	got, err := store.Get(ctx, "missing")
	require.NoError(t, err)
	assert.Nil(t, got)

	rec := &SubscriptionRecord{SubID: "0xabc", State: StateActive, Payer: "0xpayer", UpdatedAt: 100}
	require.NoError(t, store.Put(ctx, rec))

	got, err = store.Get(ctx, "0xabc")
	require.NoError(t, err)
	require.NotNil(t, got)
	assert.Equal(t, StateActive, got.State)

	// Get returns a copy; mutating it does not affect the store.
	got.State = StateCanceled
	again, _ := store.Get(ctx, "0xabc")
	assert.Equal(t, StateActive, again.State)

	require.NoError(t, store.Remove(ctx, "0xabc"))
	gone, _ := store.Get(ctx, "0xabc")
	assert.Nil(t, gone)
}

func TestIsDue(t *testing.T) {
	now := uint64(1000)
	active := &SubscriptionRecord{State: StateActive, NextChargeableAt: u64(900)}
	assert.True(t, IsDue(active, now))

	future := &SubscriptionRecord{State: StateActive, NextChargeableAt: u64(1100)}
	assert.False(t, IsDue(future, now))

	notActive := &SubscriptionRecord{State: StateCanceled, NextChargeableAt: u64(900)}
	assert.False(t, IsDue(notActive, now))

	noSchedule := &SubscriptionRecord{State: StateActive}
	assert.False(t, IsDue(noSchedule, now))
}

func TestDueSubscriptions(t *testing.T) {
	ctx := context.Background()
	store := NewInMemoryStore()
	_ = store.Put(ctx, &SubscriptionRecord{SubID: "due", State: StateActive, NextChargeableAt: u64(500), UpdatedAt: 1})
	_ = store.Put(ctx, &SubscriptionRecord{SubID: "not-due", State: StateActive, NextChargeableAt: u64(5000), UpdatedAt: 1})

	due, err := DueSubscriptions(ctx, store, 1000)
	require.NoError(t, err)
	require.Len(t, due, 1)
	assert.Equal(t, "due", due[0].SubID)
}

func TestLastChargedPeriodNilOmittedFromJSON(t *testing.T) {
	rec := &SubscriptionRecord{SubID: "x", UpdatedAt: 1}
	assert.Nil(t, rec.LastChargedPeriod)

	known := &SubscriptionRecord{SubID: "y", LastChargedPeriod: u32(0), UpdatedAt: 1}
	require.NotNil(t, known.LastChargedPeriod)
	assert.Equal(t, uint32(0), *known.LastChargedPeriod)
}
