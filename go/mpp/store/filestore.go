package store

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"unicode"
)

// FileStore is a file-system backed Store. Each key is stored as a JSON file.
// Per-key mutexes ensure concurrent operations on the same key are safe.
type FileStore[T any] struct {
	dir string
	mu  sync.Map // key -> *sync.Mutex
}

// NewFileStore creates a new FileStore that persists data in the given directory.
// Creates the directory (and parents) if it does not exist.
func NewFileStore[T any](dir string) (*FileStore[T], error) {
	if err := os.MkdirAll(dir, 0755); err != nil {
		return nil, fmt.Errorf("filestore: create directory: %w", err)
	}
	return &FileStore[T]{dir: dir}, nil
}

// sanitizeKey returns a filesystem-safe canonical form of the key.
// All locking and file I/O use this canonical form to prevent two distinct
// raw keys from aliasing to the same file with different locks.
func (f *FileStore[T]) sanitizeKey(key string) string {
	var safe []rune
	for _, r := range key {
		if unicode.IsLetter(r) || unicode.IsDigit(r) || r == '-' || r == '_' {
			safe = append(safe, r)
		} else {
			safe = append(safe, '_')
		}
	}
	return string(safe)
}

// keyLock returns the mutex for a given key, created.
// Uses the sanitized (canonical) key so that keys mapping to the same file
// share the same mutex.
func (f *FileStore[T]) keyLock(key string) *sync.Mutex {
	canonical := f.sanitizeKey(key)
	v, _ := f.mu.LoadOrStore(canonical, &sync.Mutex{})
	return v.(*sync.Mutex)
}

// keyPath returns the file path for the given key using its sanitized form.
func (f *FileStore[T]) keyPath(key string) string {
	return filepath.Join(f.dir, f.sanitizeKey(key)+".json")
}

// Get reads and unmarshals the JSON file for the given key.
// Returns nil, nil if the key does not exist.
func (f *FileStore[T]) Get(_ context.Context, key string) (*T, error) {
	mu := f.keyLock(key)
	mu.Lock()
	defer mu.Unlock()

	data, err := os.ReadFile(f.keyPath(key))
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return nil, nil
		}
		return nil, fmt.Errorf("filestore: read %q: %w", key, err)
	}
	var v T
	if err := json.Unmarshal(data, &v); err != nil {
		return nil, fmt.Errorf("filestore: unmarshal %q: %w", key, err)
	}
	return &v, nil
}

// Put marshals and writes the value as a pretty-printed JSON file.
func (f *FileStore[T]) Put(_ context.Context, key string, value *T) error {
	mu := f.keyLock(key)
	mu.Lock()
	defer mu.Unlock()

	data, err := json.Marshal(value)
	if err != nil {
		return fmt.Errorf("filestore: marshal %q: %w", key, err)
	}
	var buf bytes.Buffer
	if err := json.Indent(&buf, data, "", "  "); err != nil {
		return fmt.Errorf("filestore: format %q: %w", key, err)
	}
	buf.WriteByte('\n')
	if err := os.WriteFile(f.keyPath(key), buf.Bytes(), 0644); err != nil {
		return fmt.Errorf("filestore: write %q: %w", key, err)
	}
	return nil
}

// Delete removes the JSON file for the given key.
// No-op if the key does not exist.
func (f *FileStore[T]) Delete(_ context.Context, key string) error {
	mu := f.keyLock(key)
	mu.Lock()
	defer mu.Unlock()

	err := os.Remove(f.keyPath(key))
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		return fmt.Errorf("filestore: delete %q: %w", key, err)
	}
	return nil
}
