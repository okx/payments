// Package store provides generic key-value store interfaces and implementations.
package store

import (
	"context"
	"encoding/json"
	"fmt"
	"sync"
)

// Store is a typed key-value store interface.
type Store[T any] interface {
	Get(ctx context.Context, key string) (*T, error)
	Put(ctx context.Context, key string, value *T) error
	Delete(ctx context.Context, key string) error
}

// MemoryStore is a thread-safe, in-memory Store backed by a map.
// Values are deep-copied via JSON round-trip to prevent caller mutation.
type MemoryStore[T any] struct {
	mu   sync.RWMutex
	data map[string]json.RawMessage
}

// NewMemoryStore creates a new empty MemoryStore.
func NewMemoryStore[T any]() *MemoryStore[T] {
	return &MemoryStore[T]{data: make(map[string]json.RawMessage)}
}

// Get retrieves and unmarshals the value for the given key.
// Returns nil, nil if the key is not found.
func (m *MemoryStore[T]) Get(_ context.Context, key string) (*T, error) {
	m.mu.RLock()
	defer m.mu.RUnlock()

	raw, ok := m.data[key]
	if !ok {
		return nil, nil
	}
	var v T
	if err := json.Unmarshal(raw, &v); err != nil {
		return nil, fmt.Errorf("unmarshal %q: %w", key, err)
	}
	return &v, nil
}

// Put marshals and stores the value under the given key.
func (m *MemoryStore[T]) Put(_ context.Context, key string, value *T) error {
	data, err := json.Marshal(value)
	if err != nil {
		return fmt.Errorf("marshal %q: %w", key, err)
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	m.data[key] = data
	return nil
}

// Delete removes the value for the given key.
// No-op if the key does not exist.
func (m *MemoryStore[T]) Delete(_ context.Context, key string) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	delete(m.data, key)
	return nil
}
