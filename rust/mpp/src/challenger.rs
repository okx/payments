//! `EvmChargeChallenger` —— upstream `mpp::server::axum::ChargeChallenger` trait
//! 的 EVM + SA API 实现，让用户的 axum handler 可以直接用 `MppCharge<C>` 提取器
//! 和 `WithReceipt<T>` 响应包装，零样板。
//!
//! 上游 `impl ChargeChallenger for Mpp<TempoChargeMethod<P>, S>` 是 Tempo 的，
//! `impl ChargeChallenger for Mpp<StripeChargeMethod, S>` 是 Stripe 的。这两个
//! 都没覆盖 OKX X Layer + SA API 的场景，所以我们自己 impl。
//!
//! 上游注释明文留了这个扩展口（`src/server/axum.rs:239-240`）：
//!
//! > Implemented automatically for `Mpp<TempoChargeMethod<P>, S>` when the
//! > `tempo` feature is enabled. **Can also be implemented manually for custom
//! > payment methods.**
//!
//! # 实现要点
//!
//! - **内部持 `Mpp<EvmChargeMethod>`**：`verify_payment` 直接委托给
//!   `Mpp::verify_credential`，自动做 HMAC 校验（防 challenge_id 伪造）和
//!   过期检查 —— 跟 upstream Tempo / Stripe 的安全保证对等。
//! - **EVM 特有字段独立持有**：`currency / recipient / chain_id / fee_payer`
//!   是 EVM 后端的服务级配置，upstream `Mpp<M>` 结构里的同名字段是为 tempo
//!   helper 服务的（`Mpp::charge()` 等），EVM 后端不走那些 helper，所以我们
//!   在本结构里自己存。
//! - **`secret_key` 重复存一份用于签 challenge**：upstream `Mpp<M>.secret_key`
//!   没有 public getter，签 challenge 时我们需要一份；构造器里同时写给两处，
//!   保证一致。
//!
//! # `amount` 单位约定
//!
//! `ChargeConfig::amount()` 返回的字符串, 直接作为 `ChargeRequest.amount` 字段传给
//! 客户端, 必须是**base units 整数字符串**（MPP 协议规范硬性要求）：
//!
//! > `amount` MUST be a base-10 integer string with no sign, decimal point,
//! > exponent, or surrounding whitespace.
//!
//! 例如 pathUSD (6 decimals) 下 0.01 pathUSD 写 `"10000"`, 不是 `"0.01"`。
//!
//! upstream mpp-rs 的 doc 示例里写的是 `"0.01"`/`"1.00"` —— 那是 Tempo 后端特有的
//! 约定（`TempoChargeMethod::charge_with_options` 内部做 dollar → base units 转换），
//! 协议规范本身不允许 decimal point。本 EVM challenger 没做转换, 传进来什么就发出去什么,
//! 所以**请写 base units**。
//!
//! # 设计要点（对齐 spec §3 #5）
//!
//! - **全局 state 放在 `EvmChargeChallenger` 本身**：`currency` / `recipient` /
//!   `chain_id` / `fee_payer` / `realm` / `secret_key` 都是跨所有路由不变的
//!   服务级参数，构造时一次性传入。
//! - **per-route 参数通过 `ChargeConfig` trait 承载**：`amount` 和 `description`
//!   由各路由自己实现 `impl ChargeConfig for OneCent` 定义，由 `MppCharge<C>`
//!   在每次请求时传给 `challenge()`。
//! - 一个 `EvmChargeChallenger` 实例服务所有 MPP 路由。
//!
//! # 用法
//!
//! ## struct-literal 风格
//!
//! ```no_run
//! use std::sync::Arc;
//! use mpp::server::axum::ChargeChallenger;
//! use mpp_evm::{EvmChargeChallenger, EvmChargeChallengerConfig, EvmChargeMethod, OkxSaApiClient};
//!
//! let sa = Arc::new(OkxSaApiClient::new("k".into(), "s".into(), "p".into()));
//! let challenger = EvmChargeChallenger::new(EvmChargeChallengerConfig {
//!     charge_method: EvmChargeMethod::new(sa),
//!     currency: "0x74b7F16337b8972027F6196A17a631aC6dE26d22".into(),
//!     recipient: "0x4b22fdbc399bd422b6fefcbce95f76642ea29df1".into(),
//!     chain_id: 196,
//!     fee_payer: Some(true),
//!     realm: "photo.test".into(),
//!     secret_key: "hmac-secret".into(),
//! });
//! let _: Arc<dyn ChargeChallenger> = Arc::new(challenger);
//! ```
//!
//! ## builder 风格（对齐 upstream `Mpp::new(..).with_session_method(..)` 链式）
//!
//! ```no_run
//! use std::sync::Arc;
//! use mpp_evm::{EvmChargeChallenger, EvmChargeMethod, OkxSaApiClient};
//!
//! let sa = Arc::new(OkxSaApiClient::new("k".into(), "s".into(), "p".into()));
//! let challenger = EvmChargeChallenger::builder(EvmChargeMethod::new(sa), "photo.test", "hmac-secret")
//!     .currency("0x74b7F16337b8972027F6196A17a631aC6dE26d22")
//!     .recipient("0x4b22fdbc399bd422b6fefcbce95f76642ea29df1")
//!     .chain_id(196)
//!     .fee_payer(true)
//!     .build();
//! ```

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use mpp::protocol::core::{parse_authorization, PaymentChallenge, Receipt};
use mpp::server::axum::{ChallengeOptions, ChargeChallenger};
use mpp::server::Mpp;

use crate::challenge::{build_charge_challenge, charge_request_with};
use crate::charge_method::EvmChargeMethod;
use crate::types::ChargeMethodDetails;

/// 构造 `EvmChargeChallenger` 所需的配置（struct-literal 风格）。
///
/// 全部字段都是**跨路由共享的服务级参数**。per-route 的 `amount` / `description`
/// 不在这里 —— 那两个由用户定义的 `impl ChargeConfig for C` 提供。
pub struct EvmChargeChallengerConfig {
    /// EVM 方法实例, 内部持 SA API client (真实 `OkxSaApiClient` 或 `MockSaApiClient`)。
    pub charge_method: EvmChargeMethod,
    /// ERC-20 合约地址（收款代币），40-hex。
    pub currency: String,
    /// 收款地址，40-hex。
    pub recipient: String,
    /// 链 ID（X Layer = 196）。
    pub chain_id: u64,
    /// 是否由服务端代付 gas。`None` 表示不设置（上游默认行为）。
    pub fee_payer: Option<bool>,
    /// MPP auth realm（放在 `WWW-Authenticate: Payment realm=...`）。
    pub realm: String,
    /// 签 challenge id 的 HMAC 密钥，服务端需一致。
    pub secret_key: String,
}

/// EVM 后端 + SA API 的 `ChargeChallenger` 实现。
///
/// 用法见模块级文档。
#[derive(Clone)]
pub struct EvmChargeChallenger {
    inner: Arc<Inner>,
}

struct Inner {
    /// upstream 的 `Mpp<M>`，承担 HMAC+expiry 校验 (`verify_credential`) 和 realm/secret_key 存储。
    mpp: Mpp<EvmChargeMethod>,
    /// EVM 特有服务级配置。
    currency: String,
    recipient: String,
    chain_id: u64,
    fee_payer: Option<bool>,
    /// **重复**持一份 `secret_key` 用于签发 challenge（upstream `Mpp::secret_key` 没 public getter）。
    /// 构造器里保证跟 `mpp` 里的值一致。
    secret_key: String,
    /// **重复**持一份 `realm` 用于签 challenge（可以通过 `mpp.realm()` 读, 但放一份省一次
    /// 函数调用且增加可读性）。
    realm: String,
}

impl EvmChargeChallenger {
    /// struct-literal 风格构造。
    pub fn new(cfg: EvmChargeChallengerConfig) -> Self {
        let mpp = Mpp::new(cfg.charge_method, cfg.realm.clone(), cfg.secret_key.clone());
        Self {
            inner: Arc::new(Inner {
                mpp,
                currency: cfg.currency,
                recipient: cfg.recipient,
                chain_id: cfg.chain_id,
                fee_payer: cfg.fee_payer,
                secret_key: cfg.secret_key,
                realm: cfg.realm,
            }),
        }
    }

    /// 链式 builder 构造，对齐 upstream `Mpp::new(..).with_session_method(..)` 风格。
    pub fn builder(
        charge_method: EvmChargeMethod,
        realm: impl Into<String>,
        secret_key: impl Into<String>,
    ) -> EvmChargeChallengerBuilder {
        EvmChargeChallengerBuilder {
            charge_method,
            realm: realm.into(),
            secret_key: secret_key.into(),
            currency: None,
            recipient: None,
            chain_id: None,
            fee_payer: None,
        }
    }
}

/// `EvmChargeChallenger` 链式 builder。
///
/// **必填**：`charge_method / realm / secret_key` 通过 `builder()` 入参传入；`currency /
/// recipient / chain_id` 通过链式 setter 提供。`fee_payer` 可选。
pub struct EvmChargeChallengerBuilder {
    charge_method: EvmChargeMethod,
    realm: String,
    secret_key: String,
    currency: Option<String>,
    recipient: Option<String>,
    chain_id: Option<u64>,
    fee_payer: Option<bool>,
}

impl EvmChargeChallengerBuilder {
    pub fn currency(mut self, v: impl Into<String>) -> Self {
        self.currency = Some(v.into());
        self
    }
    pub fn recipient(mut self, v: impl Into<String>) -> Self {
        self.recipient = Some(v.into());
        self
    }
    pub fn chain_id(mut self, v: u64) -> Self {
        self.chain_id = Some(v);
        self
    }
    pub fn fee_payer(mut self, v: bool) -> Self {
        self.fee_payer = Some(v);
        self
    }

    /// 收尾。如果 `currency / recipient / chain_id` 没 set, panic（缺必填字段是编程错误）。
    pub fn build(self) -> EvmChargeChallenger {
        EvmChargeChallenger::new(EvmChargeChallengerConfig {
            charge_method: self.charge_method,
            realm: self.realm,
            secret_key: self.secret_key,
            currency: self.currency.expect("EvmChargeChallengerBuilder: currency() is required"),
            recipient: self
                .recipient
                .expect("EvmChargeChallengerBuilder: recipient() is required"),
            chain_id: self.chain_id.expect("EvmChargeChallengerBuilder: chain_id() is required"),
            fee_payer: self.fee_payer,
        })
    }
}

impl ChargeChallenger for EvmChargeChallenger {
    /// 根据 per-route 的 `amount` 和服务级 state 合成 `PaymentChallenge`。
    ///
    /// 组装链路:
    /// 1. `ChargeMethodDetails { chain_id, fee_payer, ... }`       (EVM 方法特有字段)
    /// 2. `charge_request_with(amount, currency, recipient, dtls)` (ChargeRequest, 含 method_details JSON)
    /// 3. `build_charge_challenge(secret_key, realm, &request, expires=None, description)`
    fn challenge(
        &self,
        amount: &str,
        options: ChallengeOptions,
    ) -> Result<PaymentChallenge, String> {
        let details = ChargeMethodDetails {
            chain_id: self.inner.chain_id,
            fee_payer: self.inner.fee_payer,
            permit2_address: None,
            memo: None,
            splits: None,
        };
        let request = charge_request_with(
            amount,
            &self.inner.currency,
            &self.inner.recipient,
            details,
        )?;
        build_charge_challenge(
            &self.inner.secret_key,
            &self.inner.realm,
            &request,
            None,
            options.description,
        )
    }

    /// 解析凭证串 → 委托 upstream `Mpp::verify_credential`（**自动做 HMAC 校验 + expiry
    /// 检查 + 回读 ChargeRequest + 调 method.verify**）→ 返回 `Receipt`。
    ///
    /// 相比早期直接调 `method.verify` 的实现, 这里的 HMAC 校验**防止客户端伪造
    /// challenge_id 绕过服务端签发约束**（无 HMAC 的话, 攻击者可伪造任意 challenge
    /// 把付费凭证送到 server, SA API 只验 EIP-3009 链上签名不管 challenge_id, 导致 replay 风险）。
    fn verify_payment(
        &self,
        credential_str: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Receipt, String>> + Send>> {
        // 同步阶段: parse Authorization 头, 解出 credential (含 challenge + payload)。
        let credential = match parse_authorization(credential_str) {
            Ok(c) => c,
            Err(e) => {
                return Box::pin(std::future::ready(Err(format!(
                    "parse authorization: {e}"
                ))));
            }
        };
        // 异步阶段: Mpp::verify_credential 内部做 verify_hmac_and_expiry → method.verify。
        let mpp = self.inner.mpp.clone();
        Box::pin(async move {
            mpp.verify_credential(&credential)
                .await
                .map_err(|e| e.to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockSaApiClient;
    use mpp::protocol::intents::ChargeRequest;

    fn test_challenger() -> EvmChargeChallenger {
        EvmChargeChallenger::new(EvmChargeChallengerConfig {
            charge_method: EvmChargeMethod::new(Arc::new(MockSaApiClient::new())),
            currency: "0x74b7F16337b8972027F6196A17a631aC6dE26d22".into(),
            recipient: "0x4b22fdbc399bd422b6fefcbce95f76642ea29df1".into(),
            chain_id: 196,
            fee_payer: Some(true),
            realm: "test.local".into(),
            secret_key: "test-secret".into(),
        })
    }

    #[test]
    fn challenge_yields_payment_challenge_with_evm_method() {
        let c = test_challenger();
        let ch = c
            .challenge(
                "100",
                ChallengeOptions {
                    description: Some("test item"),
                },
            )
            .expect("challenge ok");
        assert_eq!(ch.method.as_str(), "evm");
        assert_eq!(ch.intent.as_str(), "charge");
        assert_eq!(ch.realm, "test.local");
        assert_eq!(ch.description.as_deref(), Some("test item"));
        let req: ChargeRequest = ch.request.decode().unwrap();
        assert_eq!(req.amount, "100");
        assert_eq!(req.currency, "0x74b7F16337b8972027F6196A17a631aC6dE26d22");
        assert_eq!(
            req.recipient.as_deref(),
            Some("0x4b22fdbc399bd422b6fefcbce95f76642ea29df1")
        );
    }

    #[test]
    fn challenge_without_description() {
        let c = test_challenger();
        let ch = c
            .challenge("100", ChallengeOptions { description: None })
            .expect("ok");
        assert!(ch.description.is_none());
    }

    #[test]
    fn builder_yields_equivalent_challenger() {
        let sa = Arc::new(MockSaApiClient::new());
        let c = EvmChargeChallenger::builder(EvmChargeMethod::new(sa), "test.local", "test-secret")
            .currency("0x74b7F16337b8972027F6196A17a631aC6dE26d22")
            .recipient("0x4b22fdbc399bd422b6fefcbce95f76642ea29df1")
            .chain_id(196)
            .fee_payer(true)
            .build();
        let ch = c
            .challenge("100", ChallengeOptions { description: None })
            .unwrap();
        assert_eq!(ch.realm, "test.local");
    }

    #[tokio::test]
    async fn verify_bad_credential_returns_err() {
        let c = test_challenger();
        let err = c.verify_payment("not-a-payment-header").await.unwrap_err();
        assert!(err.contains("parse authorization"));
    }

    #[tokio::test]
    async fn verify_valid_mock_credential_returns_receipt() {
        let c = test_challenger();
        // 用 challenger 自己生成一个 challenge，id 是用我们 secret_key 签的，HMAC 校验能过
        let ch = c
            .challenge(
                "100",
                ChallengeOptions {
                    description: None,
                },
            )
            .unwrap();

        let credential_json = serde_json::json!({
            "challenge": {
                "id": ch.id,
                "realm": ch.realm,
                "method": "evm",
                "intent": "charge",
                "request": ch.request.raw(),
                "expires": ch.expires,
            },
            "payload": {
                "type": "transaction",
                "authorization": {
                    "type": "eip-3009",
                    "from": "0xfrom", "to": "0xto", "value": "100",
                    "validAfter": "0", "validBefore": "9999999999",
                    "nonce": "0x01", "signature": "0xsig"
                }
            }
        });
        let cred_str = serde_json::to_string(&credential_json).unwrap();
        let b64 = {
            use base64::Engine;
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(cred_str.as_bytes())
        };
        let auth_header = format!("Payment {b64}");

        let receipt = c.verify_payment(&auth_header).await.expect("verify ok");
        assert_eq!(receipt.method.as_str(), "evm");
        assert!(receipt.reference.contains("MOCK"));
    }

    /// R1 关键安全测试: 攻击者伪造 challenge.id, 不走 HMAC 签发, 应被拒绝。
    #[tokio::test]
    async fn verify_forged_challenge_id_is_rejected() {
        let c = test_challenger();
        let ch = c
            .challenge("100", ChallengeOptions { description: None })
            .unwrap();

        // 构造一个 credential, 保留 challenge 的 realm/method/intent/request/expires, 但
        // 把 id 换成攻击者自造的值。如果 verify 不做 HMAC 校验, 这会通过 (R1 bug)。
        let forged = serde_json::json!({
            "challenge": {
                "id": "attacker-forged-challenge-id",
                "realm": ch.realm,
                "method": "evm",
                "intent": "charge",
                "request": ch.request.raw(),
                "expires": ch.expires,
            },
            "payload": {
                "type": "transaction",
                "authorization": { "type": "eip-3009" }
            }
        });
        let cred_str = serde_json::to_string(&forged).unwrap();
        let b64 = {
            use base64::Engine;
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(cred_str.as_bytes())
        };
        let auth_header = format!("Payment {b64}");

        let err = c
            .verify_payment(&auth_header)
            .await
            .expect_err("forged challenge must be rejected by HMAC verify");
        // upstream Mpp::verify_hmac_and_expiry 返回的 error 消息是 "Challenge ID mismatch - not issued by this server"
        assert!(
            err.to_lowercase().contains("challenge id mismatch")
                || err.to_lowercase().contains("challenge_id")
                || err.to_lowercase().contains("not issued by this server"),
            "unexpected error message: {err}"
        );
    }
}
