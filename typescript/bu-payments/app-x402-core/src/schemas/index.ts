import { z } from "zod";

// ============================================================================
// Reusable Primitive Schemas
// ============================================================================

/**
 * Non-empty string schema - a string with at least one character.
 * Used for required string fields that cannot be empty.
 */
export const NonEmptyString = z.string().min(1);
export type NonEmptyString = z.infer<typeof NonEmptyString>;

/**
 * Any record schema - an object with unknown keys and values.
 * Used for scheme-specific payloads and other extensible objects.
 */
export const Any = z.record(z.unknown());
export type Any = z.infer<typeof Any>;

/**
 * Optional any record schema - an optional object with unknown keys and values.
 * Used for optional extension fields like `extra` and `extensions`.
 */
export const OptionalAny = z.record(z.unknown()).optional().nullable();
export type OptionalAny = z.infer<typeof OptionalAny>;

// ============================================================================
// Network Schemas
// ============================================================================

/**
 * Network identifier schema - CAIP-2 format validation.
 * Requires minimum length of 3 and a colon separator (e.g., "eip155:196", "eip155:196").
 */
export const NetworkSchema = z
  .string()
  .min(3)
  .refine(val => val.includes(":"), {
    message: "Network must be in CAIP-2 format (e.g., 'eip155:196')",
  });
export type Network = z.infer<typeof NetworkSchema>;

// ============================================================================
// Shared Schemas
// ============================================================================

/**
 * ResourceInfo schema for V2 - describes the protected resource.
 */
export const ResourceInfoSchema = z.object({
  url: NonEmptyString,
  description: z.string().optional(),
  mimeType: z.string().optional(),
});
export type ResourceInfo = z.infer<typeof ResourceInfoSchema>;

// ============================================================================
// Payment Schemas
// ============================================================================

/**
 * PaymentRequirements schema.
 */
export const PaymentRequirementsSchema = z.object({
  scheme: NonEmptyString,
  network: NetworkSchema,
  amount: NonEmptyString,
  asset: NonEmptyString,
  payTo: NonEmptyString,
  maxTimeoutSeconds: z.number().positive(),
  extra: OptionalAny,
});
export type PaymentRequirements = z.infer<typeof PaymentRequirementsSchema>;

/**
 * PaymentRequired (402 response) schema.
 * Contains payment requirements when a resource requires payment.
 */
export const PaymentRequiredSchema = z.object({
  x402Version: z.literal(2),
  error: z.string().optional(),
  resource: ResourceInfoSchema,
  accepts: z.array(PaymentRequirementsSchema).min(1),
  extensions: OptionalAny,
});
export type PaymentRequired = z.infer<typeof PaymentRequiredSchema>;

/**
 * PaymentPayload schema.
 * Contains the payment data sent by the client.
 */
export const PaymentPayloadSchema = z.object({
  x402Version: z.literal(2),
  resource: ResourceInfoSchema.optional(),
  accepted: PaymentRequirementsSchema,
  payload: Any,
  extensions: OptionalAny,
});
export type PaymentPayload = z.infer<typeof PaymentPayloadSchema>;

// ============================================================================
// Validation Functions
// ============================================================================

/**
 * Validates a PaymentRequired object (V1 or V2).
 *
 * @param value - The value to validate
 * @returns A result object with success status and data or error
 */
export function parsePaymentRequired(
  value: unknown,
): z.SafeParseReturnType<unknown, PaymentRequired> {
  return PaymentRequiredSchema.safeParse(value);
}

/**
 * Validates a PaymentRequired object and throws on error.
 *
 * @param value - The value to validate
 * @returns The validated PaymentRequired
 * @throws ZodError if validation fails
 */
export function validatePaymentRequired(value: unknown): PaymentRequired {
  return PaymentRequiredSchema.parse(value);
}

/**
 * Type guard for PaymentRequired (V1 or V2).
 *
 * @param value - The value to check
 * @returns True if the value is a valid PaymentRequired
 */
export function isPaymentRequired(value: unknown): value is PaymentRequired {
  return PaymentRequiredSchema.safeParse(value).success;
}

/**
 * Validates a PaymentRequirements object (V1 or V2).
 *
 * @param value - The value to validate
 * @returns A result object with success status and data or error
 */
export function parsePaymentRequirements(
  value: unknown,
): z.SafeParseReturnType<unknown, PaymentRequirements> {
  return PaymentRequirementsSchema.safeParse(value);
}

/**
 * Validates a PaymentRequirements object and throws on error.
 *
 * @param value - The value to validate
 * @returns The validated PaymentRequirements
 * @throws ZodError if validation fails
 */
export function validatePaymentRequirements(value: unknown): PaymentRequirements {
  return PaymentRequirementsSchema.parse(value);
}

/**
 * Type guard for PaymentRequirements (V1 or V2).
 *
 * @param value - The value to check
 * @returns True if the value is a valid PaymentRequirements
 */
export function isPaymentRequirements(value: unknown): value is PaymentRequirements {
  return PaymentRequirementsSchema.safeParse(value).success;
}

/**
 * Validates a PaymentPayload object (V1 or V2).
 *
 * @param value - The value to validate
 * @returns A result object with success status and data or error
 */
export function parsePaymentPayload(
  value: unknown,
): z.SafeParseReturnType<unknown, PaymentPayload> {
  return PaymentPayloadSchema.safeParse(value);
}

/**
 * Validates a PaymentPayload object and throws on error.
 *
 * @param value - The value to validate
 * @returns The validated PaymentPayload
 * @throws ZodError if validation fails
 */
export function validatePaymentPayload(value: unknown): PaymentPayload {
  return PaymentPayloadSchema.parse(value);
}

/**
 * Type guard for PaymentPayload (V1 or V2).
 *
 * @param value - The value to check
 * @returns True if the value is a valid PaymentPayload
 */
export function isPaymentPayload(value: unknown): value is PaymentPayload {
  return PaymentPayloadSchema.safeParse(value).success;
}

// ============================================================================
// Re-export zod for convenience
// ============================================================================

export { z } from "zod";
