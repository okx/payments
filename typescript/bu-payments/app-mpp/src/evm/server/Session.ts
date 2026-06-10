import { Errors, Method, Receipt } from "mppx";
import type { Hex, TypedDataDefinition } from "viem";
import * as Methods from "../Methods.js";
import { toChallengeEcho } from "../sa/challenge.js";
import type { SaApiClient } from "../sa/SaApiClient.js";
import type { ChallengeEcho } from "../sa/types.js";
import {
  buildCloseAuth,
  buildSettleAuth,
  DEFAULT_DOMAIN_NAME,
  DEFAULT_DOMAIN_VERSION,
  randomU256,
  unixDeadline,
  verifyVoucher,
} from "./voucher.js";

/**
 * Channel state keyed by on-chain `channelId`.
 *
 * Immutable fields (`channelId`, `chainId`, `escrowContract`, `signer`,
 * domain name/version) are set at `open` time from server-authoritative values
 * and are used for local EIP-712 voucher verification.
 */
interface ChannelState {
  /** On-chain channel id (matches `payload.channelId`). */
  channelId: Hex;
  /** EIP-712 chain id for the channel. */
  chainId: number;
  /** EIP-712 `verifyingContract` — the escrow contract address. */
  escrowContract: Hex;
  /** EIP-712 `domain.name`. */
  domainName: string;
  /** EIP-712 `domain.version`. */
  domainVersion: string;
  /** Expected voucher signer (`authorizedSigner` ?? payer). */
  signer: Hex;
  /** Current on-chain escrow deposit (from open / topUp receipt). */
  deposit: bigint;
  /** Cumulative amount already deducted locally. */
  spent: bigint;
  /** Number of charge units accumulated. */
  units: number;
  /** Highest accepted voucher amount. Available balance = `highestVoucherAmount - spent`. */
  highestVoucherAmount: bigint;
  /** Highest accepted voucher (for idempotent replay and close submission). */
  highestVoucher: { cumulativeAmount: string; signature: Hex } | null;
  /** Challenge echo for SA API calls. */
  challengeEcho: ChallengeEcho;
  /** Creation timestamp (ISO 8601). */
  createdAt: string;
}

/**
 * Seller signing capability. Both viem `LocalAccount` and
 * `WalletClient.account` satisfy this interface.
 */
export type SessionSigner = {
  signTypedData: <const td extends TypedDataDefinition>(
    params: td,
  ) => Promise<Hex>;
};

/**
 * In-place mutator for `ChannelState`. Must be synchronous and side-effect
 * free besides the mutation itself; the backend may call it multiple times
 * under contention.
 */
export type ChannelMutator = (state: ChannelState) => void;

/**
 * Channel state store. Implementations can back this with Redis, Postgres,
 * Cloudflare KV, Durable Objects, etc. Key is the on-chain `channelId`.
 *
 * `update(id, mutator)` must be atomic read-modify-write. When `state` does
 * not exist, `update` must NOT call `mutator` and must return null.
 */
export interface SessionStore {
  get(channelId: string): Promise<ChannelState | null> | ChannelState | null;
  set(channelId: string, state: ChannelState): Promise<void> | void;
  delete(channelId: string): Promise<void> | void;
  update(
    channelId: string,
    mutator: ChannelMutator,
  ): Promise<ChannelState | null> | ChannelState | null;
}

/** Default single-process in-memory store. */
export function memoryStore(): SessionStore {
  const m = new Map<string, ChannelState>();
  return {
    get: (id) => m.get(id) ?? null,
    set: (id, s) => {
      m.set(id, s);
    },
    delete: (id) => {
      m.delete(id);
    },
    update: (id, mutator) => {
      const current = m.get(id);
      if (!current) return null;
      const draft = structuredClone(current);
      mutator(draft);
      m.set(id, draft);
      return draft;
    },
  };
}

const DEFAULT_CHAIN_ID = 196; // X Layer
const DEFAULT_ESCROW_CONTRACT: Hex =
  "0x5E550002e64FaF79B41D89fE8439eEb1be66CE3b";

export function session(parameters: session.Parameters) {
  const {
    saClient,
    signer,
    store = memoryStore(),
    chainId = DEFAULT_CHAIN_ID,
    escrowContract = DEFAULT_ESCROW_CONTRACT,
    domainName = DEFAULT_DOMAIN_NAME,
    domainVersion = DEFAULT_DOMAIN_VERSION,
    minVoucherDelta: minVoucherDeltaStr = "0",
  } = parameters;
  const minVoucherDelta = BigInt(minVoucherDeltaStr);

  const locks = new Map<string, Promise<void>>();

  async function withLock<T>(
    channelId: string,
    fn: () => Promise<T> | T,
  ): Promise<T> {
    while (locks.has(channelId)) await locks.get(channelId);
    let release!: () => void;
    locks.set(
      channelId,
      new Promise<void>((r) => {
        release = r;
      }),
    );
    try {
      return await fn();
    } finally {
      locks.delete(channelId);
      release();
    }
  }

  const channels = {
    async get(id: string): Promise<ChannelState | null> {
      return (await store.get(id)) ?? null;
    },
    async set(id: string, state: ChannelState): Promise<void> {
      await store.set(id, state);
    },
    async delete(id: string): Promise<void> {
      await store.delete(id);
    },
    async update(
      id: string,
      mutator: ChannelMutator,
    ): Promise<ChannelState | null> {
      return (await store.update(id, mutator)) ?? null;
    },
  };

  async function closeChannelOnChain(
    ch: ChannelState,
    cumulativeAmount: string,
    voucherSignature: Hex,
  ) {
    const nonce = randomU256();
    const deadline = unixDeadline();
    const payeeSignature = await signer.signTypedData(
      buildCloseAuth({
        chainId: ch.chainId,
        escrowContract: ch.escrowContract,
        domainName: ch.domainName,
        domainVersion: ch.domainVersion,
        channelId: ch.channelId,
        cumulativeAmount,
        nonce,
        deadline,
      }),
    );
    return saClient.sessionClose({
      challenge: ch.challengeEcho,
      payload: {
        action: "close",
        channelId: ch.channelId,
        cumulativeAmount,
        voucherSignature,
        payeeSignature,
        nonce,
        deadline,
      },
    });
  }

  async function deduct(
    channelId: string,
    amount: bigint,
  ): Promise<ChannelState> {
    const updated = await channels.update(channelId, (ch) => {
      const available = ch.highestVoucherAmount - ch.spent;
      if (available < amount) {
        throw new Errors.InsufficientBalanceError({
          reason: "insufficient balance",
        });
      }
      ch.spent = ch.spent + amount;
      ch.units = ch.units + 1;
    });
    if (!updated)
      throw new Errors.ChannelNotFoundError({ reason: "channel not found" });
    return updated;
  }

  function resolveSigner(
    payload: Record<string, unknown>,
    source: string | undefined,
  ): Hex {
    const ZERO = "0x0000000000000000000000000000000000000000";
    const authorizedSigner = (
      payload.authorizedSigner as string | undefined
    )?.toLowerCase();
    if (authorizedSigner && authorizedSigner !== ZERO) {
      return authorizedSigner as Hex;
    }
    const fromAuth = (payload.authorization as { from?: string } | undefined)
      ?.from;
    if (fromAuth) return fromAuth as Hex;
    const didMatch = source?.match(/^did:pkh:eip155:\d+:(0x[0-9a-fA-F]{40})$/);
    if (didMatch) return didMatch[1] as Hex;
    throw new Errors.InvalidPayloadError({
      reason: "missing signer in open payload",
    });
  }

  async function verifyVoucherSig(
    ch: ChannelState,
    cumulativeAmount: string,
    signature: string,
  ): Promise<void> {
    const ok = await verifyVoucher({
      chainId: ch.chainId,
      escrowContract: ch.escrowContract,
      domainName: ch.domainName,
      domainVersion: ch.domainVersion,
      channelId: ch.channelId,
      cumulativeAmount,
      signature: signature as Hex,
      expectedSigner: ch.signer,
    }).catch(() => false);

    if (!ok) {
      throw new Errors.InvalidSignatureError({
        reason: "invalid voucher signature",
      });
    }
  }

  async function handleOpen(
    challengeEcho: ChallengeEcho,
    payload: Record<string, unknown>,
    source: string | undefined,
  ): Promise<Receipt.Receipt> {
    const channelId = payload.channelId as Hex | undefined;
    if (!channelId) {
      throw new Errors.InvalidPayloadError({
        reason: "missing channelId in open payload",
      });
    }
    return withLock(channelId, async () => {
      if (await channels.get(channelId)) {
        throw new Errors.InvalidPayloadError({
          reason: "session already exists",
        });
      }

      const voucherSigner = resolveSigner(payload, source);

      const openReceipt = await saClient.sessionOpen({
        challenge: challengeEcho,
        payload,
        source,
      } as any);

      if (
        (openReceipt.channelId as string).toLowerCase() !==
        channelId.toLowerCase()
      ) {
        throw new Errors.InvalidPayloadError({ reason: "channel id mismatch" });
      }

      const initialState: ChannelState = {
        channelId,
        chainId,
        escrowContract,
        domainName,
        domainVersion,
        signer: voucherSigner,
        deposit: BigInt(openReceipt.deposit),
        spent: 0n,
        units: 0,
        highestVoucherAmount: 0n,
        highestVoucher: null,
        challengeEcho,
        createdAt: new Date().toISOString(),
      };
      await channels.set(channelId, initialState);

      const initSignature =
        payload.type === "hash"
          ? ((payload.signature as string | undefined) ?? "")
          : ((payload.voucherSignature as string | undefined) ?? "");

      if (initSignature) {
        const initCumulativeAmount =
          (payload.cumulativeAmount as string | undefined) ?? "0";
        await verifyVoucherSig(
          initialState,
          initCumulativeAmount,
          initSignature,
        );
        const initAmount = BigInt(initCumulativeAmount);
        const accepted = await channels.update(channelId, (ch) => {
          ch.highestVoucherAmount = initAmount;
          ch.highestVoucher = {
            cumulativeAmount: initCumulativeAmount,
            signature: initSignature as Hex,
          };
        });
        if (!accepted)
          throw new Errors.ChannelNotFoundError({
            reason: "channel disappeared during open",
          });
      }

      return openReceipt as unknown as Receipt.Receipt;
    });
  }

  async function handleVoucher(
    channelId: string,
    payload: {
      action: "voucher";
      channelId: string;
      cumulativeAmount: string;
      signature: string;
      deposit?: unknown;
    },
    amount: bigint,
  ): Promise<Receipt.Receipt> {
    return withLock(channelId, async () => {
      const channel = await channels.get(channelId);
      if (!channel)
        throw new Errors.ChannelNotFoundError({ reason: "channel not found" });

      const isStoredVoucher =
        channel.highestVoucher !== null &&
        payload.cumulativeAmount === channel.highestVoucher.cumulativeAmount &&
        payload.signature === channel.highestVoucher.signature;

      if (!isStoredVoucher) {
        await verifyVoucherSig(
          channel,
          payload.cumulativeAmount,
          payload.signature,
        );

        const payloadAmount = BigInt(payload.cumulativeAmount);
        const accepted = await channels.update(channelId, (ch) => {
          if (payloadAmount < ch.highestVoucherAmount) {
            throw new Errors.InvalidPayloadError({
              reason: "voucher amount below current",
            });
          }
          if (payloadAmount > ch.deposit) {
            throw new Errors.AmountExceedsDepositError({
              reason: "voucher exceeds channel deposit",
            });
          }
          if (
            minVoucherDelta > 0n &&
            payloadAmount - ch.highestVoucherAmount < minVoucherDelta
          ) {
            throw new Errors.InvalidPayloadError({
              reason: "voucher delta below minimum",
            });
          }
          ch.highestVoucherAmount = payloadAmount;
          ch.highestVoucher = {
            cumulativeAmount: payload.cumulativeAmount,
            signature: payload.signature as Hex,
          };
        });
        if (!accepted)
          throw new Errors.ChannelNotFoundError({
            reason: "channel not found",
          });
      }

      const updated = await deduct(channelId, amount);

      return {
        method: "evm",
        intent: "session",
        status: "success",
        timestamp: new Date().toISOString(),
        challengeId: channel.challengeEcho.id,
        channelId: channel.channelId,
        spent: updated.spent.toString(),
        units: updated.units,
        chainId: channel.chainId,
      } as unknown as Receipt.Receipt;
    });
  }

  async function handleTopUp(
    channelId: string,
    payload: Record<string, unknown>,
    source: string | undefined,
  ): Promise<Receipt.Receipt> {
    return withLock(channelId, async () => {
      const channel = await channels.get(channelId);
      if (!channel)
        throw new Errors.ChannelNotFoundError({ reason: "channel not found" });

      const receipt = await saClient.sessionTopUp({
        challenge: channel.challengeEcho,
        payload,
        source,
      } as any);

      await channels.update(channelId, (ch) => {
        ch.deposit = BigInt(receipt.deposit);
      });

      return receipt as unknown as Receipt.Receipt;
    });
  }

  async function handleClose(
    channelId: string,
    payload: {
      action: "close";
      channelId: string;
      cumulativeAmount: string;
      signature: string;
    },
  ): Promise<Receipt.Receipt> {
    return withLock(channelId, async () => {
      const channel = await channels.get(channelId);
      if (!channel)
        throw new Errors.ChannelNotFoundError({ reason: "channel not found" });

      const clientSignatureRaw = payload.signature ?? "";
      const clientHasVoucher = clientSignatureRaw !== "";
      const clientSignature = clientSignatureRaw as Hex;
      if (clientHasVoucher) {
        await verifyVoucherSig(
          channel,
          payload.cumulativeAmount,
          clientSignature,
        );
      }

      const clientAmount = clientHasVoucher
        ? BigInt(payload.cumulativeAmount)
        : -1n;
      const storedAmount = channel.highestVoucher
        ? channel.highestVoucherAmount
        : -1n;

      if (
        clientHasVoucher &&
        channel.highestVoucher &&
        clientAmount < storedAmount
      ) {
        throw new Errors.InvalidPayloadError({
          reason: "voucher amount below current",
        });
      }

      let cumulativeAmount: string;
      let voucherSignature: Hex;
      if (clientAmount < 0n && storedAmount < 0n) {
        cumulativeAmount = "0";
        voucherSignature = "" as Hex;
      } else if (clientHasVoucher) {
        cumulativeAmount = payload.cumulativeAmount;
        voucherSignature = clientSignature;
      } else {
        cumulativeAmount = channel.highestVoucher!.cumulativeAmount;
        voucherSignature = channel.highestVoucher!.signature;
      }

      const receipt = await closeChannelOnChain(
        channel,
        cumulativeAmount,
        voucherSignature,
      );

      await channels.delete(channelId);

      return receipt as unknown as Receipt.Receipt;
    });
  }

  const method = Method.toServer(Methods.session, {
    defaults: {
      methodDetails: {
        chainId,
        escrowContract,
      },
    },

    async request({ request }) {
      return request;
    },

    async verify({ credential, request }) {
      const { payload } = credential;
      const source = credential.source;
      const requestObj = request as Record<string, unknown>;
      const amount = BigInt(requestObj.amount as string);
      const challengeEcho = toChallengeEcho(
        credential.challenge as Record<string, unknown>,
        requestObj,
      );

      switch (payload.action) {
        case "open":
          return handleOpen(challengeEcho, payload, source);
        case "voucher":
          return handleVoucher(payload.channelId, payload, amount);
        case "topUp":
          return handleTopUp(payload.channelId, payload, source);
        case "close":
          return handleClose(payload.channelId, payload);
        default:
          throw new Errors.InvalidPayloadError({
            reason: "unsupported session action",
          });
      }
    },

    respond({ credential }) {
      const action = (credential.payload as { action: string }).action;
      // open / topUp / close are management actions: no resource body, only the
      // receipt header is needed.
      if (action === "open" || action === "topUp" || action === "close") {
        return new Response(null, { status: 204 });
      }
      return undefined;
    },
  });

  /**
   * Mid-session settle: submit the current highest accepted voucher on chain
   * without closing the channel. The seller signs a `SettleAuthorization`.
   */
  async function settle(channelId: string) {
    return withLock(channelId, async () => {
      const channel = await channels.get(channelId);
      if (!channel)
        throw new Errors.ChannelNotFoundError({ reason: "channel not found" });
      if (!channel.highestVoucher) {
        throw new Errors.InvalidPayloadError({
          reason: "no voucher to settle",
        });
      }

      const cumulativeAmount = channel.highestVoucherAmount.toString();
      const voucherSignature = channel.highestVoucher.signature;
      const nonce = randomU256();
      const deadline = unixDeadline();
      const payeeSignature = await signer.signTypedData(
        buildSettleAuth({
          chainId: channel.chainId,
          escrowContract: channel.escrowContract,
          domainName: channel.domainName,
          domainVersion: channel.domainVersion,
          channelId: channel.channelId,
          cumulativeAmount,
          nonce,
          deadline,
        }),
      );

      return saClient.sessionSettle({
        challenge: channel.challengeEcho,
        payload: {
          action: "settle",
          channelId: channel.channelId,
          cumulativeAmount,
          voucherSignature,
          payeeSignature,
          nonce,
          deadline,
        },
      });
    });
  }

  /** Read-only channel status. The argument is the on-chain `channelId`. */
  async function status(channelId: string) {
    return saClient.sessionStatus(channelId);
  }

  return Object.assign(method, { settle, status });
}

export declare namespace session {
  type Parameters = {
    /** SA API client instance (handles on-chain open / topUp / settle / close). */
    saClient: SaApiClient;
    /**
     * Seller signer used to sign EIP-712 authorizations for settle / close.
     * Must implement `signTypedData(typedData)`; viem `LocalAccount` and
     * `WalletClient.account` both satisfy this.
     */
    signer: SessionSigner;
    /**
     * EIP-712 chain id. Defaults to 196 (X Layer). Immutable for the lifetime
     * of a channel.
     */
    chainId?: number;
    /**
     * `EvmPaymentChannel` escrow contract address (EIP-712 `verifyingContract`).
     * Must be the real deployed contract address; otherwise every voucher
     * signature will fail verification.
     */
    escrowContract?: Hex;
    /**
     * EIP-712 `domain.name`, defaults to "EVM Payment Channel". Must match the
     * on-chain escrow contract's `EIP712Domain` exactly.
     */
    domainName?: string;
    /**
     * EIP-712 `domain.version`, defaults to "1". Must match the on-chain
     * escrow contract's `EIP712Domain` exactly.
     */
    domainVersion?: string;
    /**
     * Channel state store. Defaults to an in-process memory store.
     * Implements `{ get, set, delete, update }`; key is the on-chain
     * `channelId`. The SDK serializes per-`channelId` access internally, so
     * remote / persistent stores (Redis, KV, SQL) can plug in directly.
     */
    store?: SessionStore;
    /** Minimum voucher delta (base units, stringified). Defaults to `"0"`. */
    minVoucherDelta?: string;
  };
}
