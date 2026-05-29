package paymentrouter

import (
	"context"
	"net/http"
)

type ProtocolAdapter interface {
	Name() string
	Priority() int
	Detect(r *http.Request) bool
	GetChallenge(ctx context.Context, r *http.Request, cfg any) (http.Header, error)
	Handle(w http.ResponseWriter, r *http.Request, cfg any) error
}
