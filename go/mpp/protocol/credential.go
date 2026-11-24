package protocol

// ─────────────────────────────────────────────────────────────────────────────
// PaymentPayload
// ─────────────────────────────────────────────────────────────────────────────

// PaymentPayload holds the type and encoded body of a payment payload attached
// to a PaymentCredential.
type PaymentPayload struct {
	Type    PayloadType `json:"payload_type"`
	Payload string      `json:"payload"`
}

// NewTransactionPayload creates a PaymentPayload with type "transaction".
// The payload must be a valid JSON string (raw payload object).
func NewTransactionPayload(payload string) *PaymentPayload {
	return &PaymentPayload{Type: PayloadTypeTransaction, Payload: payload}
}

// NewHashPayload creates a PaymentPayload with type "hash".
// The payload must be a valid JSON string (raw payload object).
func NewHashPayload(payload string) *PaymentPayload {
	return &PaymentPayload{Type: PayloadTypeHash, Payload: payload}
}

// NewProofPayload creates a PaymentPayload with type "proof".
// The payload must be a valid JSON string (raw payload object).
func NewProofPayload(payload string) *PaymentPayload {
	return &PaymentPayload{Type: PayloadTypeProof, Payload: payload}
}

// ─────────────────────────────────────────────────────────────────────────────
// PaymentCredential
// ─────────────────────────────────────────────────────────────────────────────

// PaymentCredential holds the data submitted in an Authorization: Payment
// header (base64url-encoded JSON blob).
type PaymentCredential struct {
	Echo    *ChallengeEcho  `json:"echo"`
	Source  string          `json:"source,omitempty"`
	Payload *PaymentPayload `json:"payload"`
}

// NewPaymentCredential creates a PaymentCredential without a source DID.
func NewPaymentCredential(echo *ChallengeEcho, payload *PaymentPayload) *PaymentCredential {
	return &PaymentCredential{Echo: echo, Payload: payload}
}

// NewPaymentCredentialWithSource creates a PaymentCredential that includes a
// source DID.
func NewPaymentCredentialWithSource(echo *ChallengeEcho, source string, payload *PaymentPayload) *PaymentCredential {
	return &PaymentCredential{Echo: echo, Source: source, Payload: payload}
}
