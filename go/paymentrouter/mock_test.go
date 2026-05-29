package paymentrouter

import (
	"context"
	"net/http"
)

type mockAdapter struct {
	name         string
	priority     int
	detectResult bool
	detectPanic  bool
	challengeH   http.Header
	challengeErr error
	handleErr    error
}

func (m *mockAdapter) Name() string     { return m.name }
func (m *mockAdapter) Priority() int    { return m.priority }
func (m *mockAdapter) Detect(_ *http.Request) bool {
	if m.detectPanic {
		panic("mock panic")
	}
	return m.detectResult
}
func (m *mockAdapter) GetChallenge(_ context.Context, _ *http.Request, _ any) (http.Header, error) {
	return m.challengeH, m.challengeErr
}
func (m *mockAdapter) Handle(_ http.ResponseWriter, _ *http.Request, _ any) error {
	return m.handleErr
}
