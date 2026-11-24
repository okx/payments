package saclient

import "context"

// SAClient is the interface for interacting with the SA API.
// The default implementation is OKXSAClient, which targets the OKX SA API.
// Developers may provide custom implementations for testing or alternative backends.
type SAClient interface {
	// Charge (client-facing — forward credential)
	Settle(ctx context.Context, req *ChargeSettleRequest) (*ChargeReceipt, error)
	VerifyHash(ctx context.Context, req *ChargeVerifyHashRequest) (*ChargeReceipt, error)

	// Session (client-facing — forward credential)
	SessionOpen(ctx context.Context, req *SessionOpenRequest) (*SessionReceipt, error)
	SessionTopUp(ctx context.Context, req *SessionTopUpRequest) (*SessionReceipt, error)

	// Session (merchant-facing — server builds request)
	SessionSettle(ctx context.Context, req *SessionSettleRequest) (*SessionReceipt, error)
	SessionClose(ctx context.Context, req *SessionCloseRequest) (*SessionReceipt, error)

	// Session (read-only)
	SessionStatus(ctx context.Context, channelID string) (*SessionStatus, error)
}
