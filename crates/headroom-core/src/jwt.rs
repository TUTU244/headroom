//! JWT bearer-token expiry helpers.
//!
//! We **never** validate signatures here — the upstream LLM gateway is
//! authoritative. We only decode the payload segment to read the `exp`
//! claim so the proxy can return an actionable 401 before forwarding a
//! clearly-expired token, rather than letting the request race to the
//! upstream and getting an opaque disconnect.
//!
//! # Why not a full JWK validation?
//!
//! JWK validation would require knowing the issuer's well-known endpoint,
//! fetching the public key set, and running RS256/ES256 verification. That
//! couples the proxy to identity-provider specifics and adds latency on
//! the hot path. The `exp` check alone covers the common disconnect case
//! (token expired while the NLM CLI session was idle), and incorrect `exp`
//! values are caught by the upstream anyway.
//!
//! # No new dependencies
//!
//! base64url decoding is done with a small inline lookup table. JSON
//! parsing reuses `serde_json` already in the dep tree.

use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds before JWT expiry at which we consider the token "close to
/// expiry". Consistent with `REFRESH_AHEAD_SECS` in `vertex/adc.rs` so
/// the policy is uniform across all token-bearing paths.
pub const REFRESH_AHEAD_SECS: u64 = 60;

/// Result of inspecting a JWT's `exp` claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JwtExpiry {
    /// Token has expired (`exp <= now`). Return 401 early.
    Expired,
    /// Token is valid; this many seconds remain before expiry.
    ValidFor(u64),
    /// No `exp` claim, un-decodable payload, or not a JWT shape.
    /// Pass the request through and let the upstream decide.
    Unknown,
}

impl JwtExpiry {
    /// True when the token is already past its `exp` timestamp.
    pub fn is_expired(self) -> bool {
        matches!(self, JwtExpiry::Expired)
    }

    /// True when the token is expired or will expire within
    /// [`REFRESH_AHEAD_SECS`]. Use this to emit early-warning logs.
    pub fn is_close_to_expiry(self) -> bool {
        match self {
            JwtExpiry::Expired => true,
            JwtExpiry::ValidFor(secs) => secs < REFRESH_AHEAD_SECS,
            JwtExpiry::Unknown => false,
        }
    }
}

/// Extract the `exp` claim from a compact JWT (`header.payload.signature`).
///
/// Returns `JwtExpiry::Unknown` on any parse or decode error so the caller
/// always falls through to the upstream rather than hard-failing on a
/// non-JWT bearer shape (e.g. opaque API key that coincidentally has dots).
pub fn read_expiry(token: &str) -> JwtExpiry {
    let payload_b64 = match token.split('.').nth(1) {
        Some(s) if !s.is_empty() => s,
        _ => return JwtExpiry::Unknown,
    };

    let payload_bytes = match decode_base64url(payload_b64) {
        Some(b) => b,
        None => return JwtExpiry::Unknown,
    };

    let payload: serde_json::Value = match serde_json::from_slice(&payload_bytes) {
        Ok(v) => v,
        Err(_) => return JwtExpiry::Unknown,
    };

    let exp = match payload.get("exp").and_then(|v| v.as_u64()) {
        Some(e) => e,
        None => return JwtExpiry::Unknown,
    };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    if exp <= now {
        JwtExpiry::Expired
    } else {
        JwtExpiry::ValidFor(exp - now)
    }
}

// ── base64url decode ─────────────────────────────────────────────────────────
// RFC 4648 §5 alphabet; padding is optional (JWTs omit it).
// We build the 256-byte decode table at compile time with a const fn so
// there's no runtime initialisation cost.

const fn make_decode_table() -> [u8; 256] {
    let mut t = [0xFFu8; 256];
    let mut i = 0usize;
    // A–Z → 0–25
    while i < 26 {
        t[b'A' as usize + i] = i as u8;
        i += 1;
    }
    // a–z → 26–51
    i = 0;
    while i < 26 {
        t[b'a' as usize + i] = (26 + i) as u8;
        i += 1;
    }
    // 0–9 → 52–61
    i = 0;
    while i < 10 {
        t[b'0' as usize + i] = (52 + i) as u8;
        i += 1;
    }
    // standard alphabet: + → 62, / → 63
    t[b'+' as usize] = 62;
    t[b'/' as usize] = 63;
    // URL-safe alphabet: - → 62, _ → 63
    t[b'-' as usize] = 62;
    t[b'_' as usize] = 63;
    t
}

static DECODE_TABLE: [u8; 256] = make_decode_table();

/// Decode a base64url string (with or without `=` padding) into bytes.
/// Returns `None` on any invalid character.
fn decode_base64url(s: &str) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=');
    let n = s.len();
    let out_capacity = (n * 3 + 3) / 4;
    let mut out = Vec::with_capacity(out_capacity);
    let bytes = s.as_bytes();
    let mut i = 0;

    // Full 4-char groups.
    while i + 4 <= n {
        let a = DECODE_TABLE[bytes[i] as usize];
        let b = DECODE_TABLE[bytes[i + 1] as usize];
        let c = DECODE_TABLE[bytes[i + 2] as usize];
        let d = DECODE_TABLE[bytes[i + 3] as usize];
        if a == 0xFF || b == 0xFF || c == 0xFF || d == 0xFF {
            return None;
        }
        out.push((a << 2) | (b >> 4));
        out.push(((b & 0x0F) << 4) | (c >> 2));
        out.push(((c & 0x03) << 6) | d);
        i += 4;
    }

    // Tail: 2 or 3 remaining characters (1 remaining char is invalid).
    match n - i {
        0 => {}
        2 => {
            let a = DECODE_TABLE[bytes[i] as usize];
            let b = DECODE_TABLE[bytes[i + 1] as usize];
            if a == 0xFF || b == 0xFF {
                return None;
            }
            out.push((a << 2) | (b >> 4));
        }
        3 => {
            let a = DECODE_TABLE[bytes[i] as usize];
            let b = DECODE_TABLE[bytes[i + 1] as usize];
            let c = DECODE_TABLE[bytes[i + 2] as usize];
            if a == 0xFF || b == 0xFF || c == 0xFF {
                return None;
            }
            out.push((a << 2) | (b >> 4));
            out.push(((b & 0x0F) << 4) | (c >> 2));
        }
        _ => return None,
    }

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Build a minimal JWT with a given `exp` claim (no real signing — we never
    // validate signatures, so the signature segment can be anything).
    fn make_jwt(exp_offset_secs: i64) -> String {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let exp = now + exp_offset_secs;
        // header: {"alg":"none"}
        let header = base64url_encode(br#"{"alg":"none"}"#);
        // payload with exp
        let payload_json = format!(r#"{{"sub":"nlm-test","exp":{exp}}}"#);
        let payload = base64url_encode(payload_json.as_bytes());
        format!("{header}.{payload}.fakesig")
    }

    fn make_jwt_no_exp() -> String {
        let header = base64url_encode(br#"{"alg":"none"}"#);
        let payload = base64url_encode(br#"{"sub":"no-exp"}"#);
        format!("{header}.{payload}.fakesig")
    }

    fn base64url_encode(input: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        let mut i = 0;
        while i + 3 <= input.len() {
            let a = input[i] as usize;
            let b = input[i + 1] as usize;
            let c = input[i + 2] as usize;
            out.push(TABLE[a >> 2] as char);
            out.push(TABLE[((a & 3) << 4) | (b >> 4)] as char);
            out.push(TABLE[((b & 0xF) << 2) | (c >> 6)] as char);
            out.push(TABLE[c & 0x3F] as char);
            i += 3;
        }
        match input.len() - i {
            1 => {
                let a = input[i] as usize;
                out.push(TABLE[a >> 2] as char);
                out.push(TABLE[(a & 3) << 4] as char);
            }
            2 => {
                let a = input[i] as usize;
                let b = input[i + 1] as usize;
                out.push(TABLE[a >> 2] as char);
                out.push(TABLE[((a & 3) << 4) | (b >> 4)] as char);
                out.push(TABLE[(b & 0xF) << 2] as char);
            }
            _ => {}
        }
        out
    }

    #[test]
    fn expired_token_detected() {
        let jwt = make_jwt(-10); // expired 10 s ago
        assert_eq!(read_expiry(&jwt), JwtExpiry::Expired);
        assert!(read_expiry(&jwt).is_expired());
    }

    #[test]
    fn valid_token_reports_remaining() {
        let jwt = make_jwt(3600); // 1 h from now
        match read_expiry(&jwt) {
            JwtExpiry::ValidFor(secs) => {
                assert!(secs > 3500 && secs <= 3600, "expected ~3600, got {secs}");
            }
            other => panic!("expected ValidFor, got {other:?}"),
        }
    }

    #[test]
    fn close_to_expiry_within_60s() {
        let jwt = make_jwt(30); // 30 s, below REFRESH_AHEAD_SECS
        assert!(read_expiry(&jwt).is_close_to_expiry());
    }

    #[test]
    fn no_exp_claim_is_unknown() {
        let jwt = make_jwt_no_exp();
        assert_eq!(read_expiry(&jwt), JwtExpiry::Unknown);
    }

    #[test]
    fn non_jwt_string_is_unknown() {
        assert_eq!(read_expiry("sk-ant-api-notajwt"), JwtExpiry::Unknown);
        assert_eq!(read_expiry(""), JwtExpiry::Unknown);
    }

    #[test]
    fn garbage_payload_is_unknown() {
        // header is valid base64url but payload is not JSON
        let header = base64url_encode(br#"{"alg":"none"}"#);
        let bad_payload = base64url_encode(b"not-json-at-all!!!");
        let jwt = format!("{header}.{bad_payload}.sig");
        assert_eq!(read_expiry(&jwt), JwtExpiry::Unknown);
    }

    #[test]
    fn decode_base64url_roundtrip() {
        // All 256 byte values, padded to a length divisible by 3 for simplicity.
        let data: Vec<u8> = (0u8..=255).collect();
        let encoded = base64url_encode(&data);
        let decoded = decode_base64url(&encoded).expect("decode must succeed");
        assert_eq!(decoded, data);
    }
}
