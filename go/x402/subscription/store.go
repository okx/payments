package subscription

import (
	"context"
	"sync"
)

// SubscriptionRecord is the seller-side cached view of a subscription. The
// facilitator and chain remain authoritative; this record backs the access
// fast path and the due-charge scan. The current period is never cached — it is
// recomputed from the clock on every access.
type SubscriptionRecord struct {
	SubID            string  `json:"subId"`
	State            uint8   `json:"state"`
	Payer            string  `json:"payer"`
	PlanID           string  `json:"planId,omitempty"`
	PlanTier         uint8   `json:"planTier,omitempty"`
	NextChargeableAt *uint64 `json:"nextChargeableAt,omitempty"`
	ChangedToSubID   *string `json:"changedToSubId,omitempty"`
	StartAt          uint64  `json:"startAt,omitempty"`
	PeriodSec        uint64  `json:"periodSec,omitempty"`
	PeriodMode       uint8   `json:"periodMode,omitempty"`
	BillingAnchorAt  uint64  `json:"billingAnchorAt,omitempty"`
	MaxPeriods       uint32  `json:"maxPeriods,omitempty"`
	// LastChargedPeriod is nil when unknown (never fetched): that forces the
	// access slow path. A non-nil pointer to 0 means known-never-charged.
	LastChargedPeriod *uint32 `json:"lastChargedPeriod,omitempty"`
	UpdatedAt         uint64  `json:"updatedAt"`
}

// IsActive reports whether the record is in the ACTIVE state.
func (r *SubscriptionRecord) IsActive() bool {
	return r.State == StateActive
}

// SubscriptionStore is a pure key-value cache of subscription records keyed by
// subId. The in-memory implementation ships with the SDK; Redis/SQL/file
// persistence is the seller's extension point.
type SubscriptionStore interface {
	// Get returns the record for subId, or (nil, nil) when absent.
	Get(ctx context.Context, subID string) (*SubscriptionRecord, error)
	Put(ctx context.Context, record *SubscriptionRecord) error
	Remove(ctx context.Context, subID string) error
	List(ctx context.Context) ([]*SubscriptionRecord, error)
}

// IsDue reports whether an active record is due for a charge at now.
func IsDue(rec *SubscriptionRecord, now uint64) bool {
	return rec.IsActive() && rec.NextChargeableAt != nil && *rec.NextChargeableAt <= now
}

// DueSubscriptions returns every record in the store that is due for a charge at
// now. An empty store (or nil) yields no work.
func DueSubscriptions(ctx context.Context, store SubscriptionStore, now uint64) ([]*SubscriptionRecord, error) {
	if store == nil {
		return nil, nil
	}
	records, err := store.List(ctx)
	if err != nil {
		return nil, err
	}
	due := make([]*SubscriptionRecord, 0, len(records))
	for _, rec := range records {
		if IsDue(rec, now) {
			due = append(due, rec)
		}
	}
	return due, nil
}

// InMemoryStore is a process-local, mutex-guarded SubscriptionStore. It is not
// durable: records are lost on restart.
type InMemoryStore struct {
	mu      sync.RWMutex
	records map[string]*SubscriptionRecord
}

// NewInMemoryStore creates an empty in-memory store.
func NewInMemoryStore() *InMemoryStore {
	return &InMemoryStore{records: make(map[string]*SubscriptionRecord)}
}

// Get returns a copy of the stored record, or (nil, nil) if absent.
func (s *InMemoryStore) Get(_ context.Context, subID string) (*SubscriptionRecord, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	rec, ok := s.records[subID]
	if !ok {
		return nil, nil
	}
	clone := *rec
	return &clone, nil
}

// Put stores a copy of the record, overwriting any prior entry for its subId.
func (s *InMemoryStore) Put(_ context.Context, record *SubscriptionRecord) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	clone := *record
	s.records[record.SubID] = &clone
	return nil
}

// Remove deletes the record for subId if present.
func (s *InMemoryStore) Remove(_ context.Context, subID string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	delete(s.records, subID)
	return nil
}

// List returns copies of all stored records in unspecified order.
func (s *InMemoryStore) List(_ context.Context) ([]*SubscriptionRecord, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	out := make([]*SubscriptionRecord, 0, len(s.records))
	for _, rec := range s.records {
		clone := *rec
		out = append(out, &clone)
	}
	return out, nil
}
