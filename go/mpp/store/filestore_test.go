package store

import (
	"context"
	"math/big"
	"os"
	"path/filepath"
	"testing"
)

type testData struct {
	Name  string `json:"name"`
	Value int    `json:"value"`
}

func TestFileStore_PutGet(t *testing.T) {
	dir := t.TempDir()
	fs, err := NewFileStore[testData](dir)
	if err != nil {
		t.Fatalf("NewFileStore: %v", err)
	}

	ctx := context.Background()
	if err := fs.Put(ctx, "k1", &testData{Name: "hello", Value: 42}); err != nil {
		t.Fatalf("Put: %v", err)
	}

	got, err := fs.Get(ctx, "k1")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if got == nil || got.Name != "hello" || got.Value != 42 {
		t.Errorf("Get: got %+v", got)
	}
}

func TestFileStore_GetMissing(t *testing.T) {
	fs, _ := NewFileStore[testData](t.TempDir())
	got, err := fs.Get(context.Background(), "missing")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if got != nil {
		t.Errorf("expected nil for missing key")
	}
}

func TestFileStore_Delete(t *testing.T) {
	dir := t.TempDir()
	fs, _ := NewFileStore[testData](dir)
	ctx := context.Background()

	fs.Put(ctx, "k1", &testData{Name: "x"})
	if err := fs.Delete(ctx, "k1"); err != nil {
		t.Fatalf("Delete: %v", err)
	}

	got, _ := fs.Get(ctx, "k1")
	if got != nil {
		t.Error("expected nil after delete")
	}
}

func TestFileStore_DeleteMissing(t *testing.T) {
	fs, _ := NewFileStore[testData](t.TempDir())
	err := fs.Delete(context.Background(), "nope")
	if err != nil {
		t.Errorf("Delete missing: %v", err)
	}
}

func TestFileStore_KeySanitization(t *testing.T) {
	dir := t.TempDir()
	fs, _ := NewFileStore[testData](dir)
	ctx := context.Background()

	fs.Put(ctx, "0x1234:abc", &testData{Name: "sanitized"})

	// File should exist with sanitized name.
	matches, _ := filepath.Glob(filepath.Join(dir, "*.json"))
	if len(matches) != 1 {
		t.Fatalf("expected 1 file, got %d", len(matches))
	}

	got, _ := fs.Get(ctx, "0x1234:abc")
	if got == nil || got.Name != "sanitized" {
		t.Errorf("round-trip failed: %+v", got)
	}
}

func TestFileStore_PrettyPrint(t *testing.T) {
	dir := t.TempDir()
	fs, _ := NewFileStore[testData](dir)
	fs.Put(context.Background(), "pp", &testData{Name: "pretty", Value: 1})

	matches, _ := filepath.Glob(filepath.Join(dir, "*.json"))
	if len(matches) != 1 {
		t.Fatal("expected 1 file")
	}
	data, _ := os.ReadFile(matches[0])
	if len(data) < 10 {
		t.Error("file too short for pretty print")
	}
}

func TestFileStore_ChannelState(t *testing.T) {
	dir := t.TempDir()
	fs, _ := NewFileStore[ChannelState](dir)
	ctx := context.Background()

	cs := &ChannelState{
		ChannelID: "0xabc",
		Deposit:   big.NewInt(1000000),
		Spent:     big.NewInt(0),
	}
	if err := fs.Put(ctx, cs.ChannelID, cs); err != nil {
		t.Fatalf("Put: %v", err)
	}

	got, err := fs.Get(ctx, cs.ChannelID)
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if got.ChannelID != "0xabc" {
		t.Errorf("ChannelID: %q", got.ChannelID)
	}
	if got.Deposit.Cmp(big.NewInt(1000000)) != 0 {
		t.Errorf("Deposit: %s", got.Deposit)
	}
}
