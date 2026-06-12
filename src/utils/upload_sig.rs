//! Signs the R2 storage keys handed to the upload Worker so it can verify the
//! key was issued by us (the Worker shares `UPLOAD_SIGNING_SECRET`). Without
//! this, anyone could `PUT` arbitrary objects into the bucket via the Worker.

use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

const TTL_SECONDS: i64 = 3600;

fn hmac_hex(secret: &str, msg: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts a key of any length");
    mac.update(msg.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Returns `<exp>.<hmac_hex>` binding `storage_key` to a short expiry, where
/// `hmac = HMAC-SHA256(secret, "<storage_key>:<exp>")`. Returns an empty string
/// when no secret is configured (signing disabled / not yet rolled out).
pub fn sign_storage_key(secret: &str, storage_key: &str) -> String {
    if secret.is_empty() {
        return String::new();
    }
    let exp = Utc::now().timestamp() + TTL_SECONDS;
    let sig = hmac_hex(secret, &format!("{}:{}", storage_key, exp));
    format!("{}.{}", exp, sig)
}

#[cfg(test)]
mod tests {
    use super::hmac_hex;

    // Cross-language vector: must equal Node `crypto.createHmac` / WebCrypto /
    // openssl for the same (secret, message). Guards the Worker<->backend match.
    #[test]
    fn hmac_matches_known_vector() {
        assert_eq!(
            hmac_hex("test-secret", "uploads/abc.pdf:1700000000"),
            "dee6f42934b7ba703c24c843edc0f85876174b433ff8195d9046369e1a51f1c1"
        );
    }
}
