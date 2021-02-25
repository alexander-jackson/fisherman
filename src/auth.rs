use actix_web::HttpResponse;
use hmac::{Hmac, Mac, NewMac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub fn validate_webhook_body(
    bytes: &[u8],
    secret: Option<&[u8]>,
    expected: Option<&[u8]>,
) -> Result<(), HttpResponse> {
    let (secret, expected) = match (secret, expected) {
        (Some(secret), Some(expected)) => (secret, hex::decode(expected).unwrap()),
        (Some(_), None) => {
            return Err(HttpResponse::BadRequest().body("Found a secret but no expected value"))
        }
        (None, Some(_)) => {
            return Err(HttpResponse::BadRequest().body("Found an expected value but no secret"))
        }
        (None, None) => return Ok(()),
    };

    let mut mac = HmacSha256::new_varkey(secret).expect("HMAC can take key of any size");

    mac.update(bytes);
    mac.verify(&expected)
        .map_err(|_| HttpResponse::Unauthorized().body("Secret failed to authorise the payload"))
}

#[cfg(test)]
mod tests {
    use crate::auth::validate_webhook_body;

    static SAMPLE_PAYLOAD: &[u8] = include_bytes!("../sample_payload.json");

    #[test]
    fn missing_secret_and_expected_allows_access() {
        assert!(validate_webhook_body(b"", None, None).is_ok());
    }

    #[test]
    fn secret_but_not_expected_fails_authentication() {
        assert!(validate_webhook_body(b"", Some(b""), None).is_err());
    }

    #[test]
    fn missing_secret_but_expected_fails_authentication() {
        assert!(validate_webhook_body(b"", None, Some(b"")).is_err());
    }

    #[test]
    fn correct_payloads_are_validated() {
        let secret = Some("ac9045a77c15bd105cfa09a64635f9b006b3f845".as_bytes());
        let expected =
            Some("9e31091766db83d80ec93c84b24158d54839482e5566c1dfbe0dca45cfdc330b".as_bytes());

        assert!(validate_webhook_body(SAMPLE_PAYLOAD, secret, expected).is_ok());
    }
}