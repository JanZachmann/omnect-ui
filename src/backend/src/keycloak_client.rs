use crate::config::AppConfig;
use anyhow::{Context, Result};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
#[cfg(feature = "mock")]
use mockall::automock;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use trait_variant::make;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TokenClaims {
    pub roles: Option<Vec<String>>,
    pub tenant_list: Option<Vec<String>>,
    pub fleet_list: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct RealmInfo {
    public_key: String,
}

#[make(Send + Sync)]
#[cfg_attr(feature = "mock", automock)]
pub trait SingleSignOnProvider {
    async fn verify_token(&self, token: &str) -> anyhow::Result<TokenClaims>;
}

#[derive(Clone, Default)]
pub struct KeycloakProvider;

impl KeycloakProvider {
    pub fn create_frontend_config_file() -> Result<()> {
        use anyhow::Context;
        use std::io::Write;

        let mut config_file = std::fs::File::create(&AppConfig::get().paths.app_config_path)
            .context("failed to create frontend config file")?;

        config_file
            .write_all(
                format!(
                    "window.__APP_CONFIG__ = {{KEYCLOAK_URL:\"{}\"}};",
                    AppConfig::get().keycloak.url
                )
                .as_bytes(),
            )
            .context("failed to write frontend config file")
    }

    async fn realm_public_key(&self) -> Result<DecodingKey> {
        let client = Client::new();
        let resp = client
            .get(&AppConfig::get().keycloak.url)
            .send()
            .await
            .context("failed to fetch from url")?
            .json::<RealmInfo>()
            .await
            .context("failed to parse realm info")?;

        decoding_key_from_keycloak(&resp.public_key)
    }
}

/// Parse base64-encoded SPKI public key (as returned by Keycloak's realm endpoint)
/// into a `DecodingKey` for JWT verification.
fn decoding_key_from_keycloak(base64_spki: &str) -> Result<DecodingKey> {
    let pem = format!("-----BEGIN PUBLIC KEY-----\n{base64_spki}\n-----END PUBLIC KEY-----");
    DecodingKey::from_rsa_pem(pem.as_bytes()).context("failed to parse public key from PEM")
}

impl SingleSignOnProvider for KeycloakProvider {
    async fn verify_token(&self, token: &str) -> anyhow::Result<TokenClaims> {
        let pub_key = self.realm_public_key().await?;
        let mut validation = Validation::new(Algorithm::RS256);
        // Keycloak tokens usually have these, but we don't strictly require them here
        // as we only care about the custom claims if the token is valid.
        validation.validate_exp = true;
        validation.validate_aud = false;
        validation.required_spec_claims.remove("iss");
        validation.required_spec_claims.remove("aud");

        let claims = decode::<TokenClaims>(token, &pub_key, &validation)?;
        Ok(claims.claims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{EncodingKey, Header, encode, get_current_timestamp};

    // Pre-generated RSA 2048-bit test keypair (PKCS#8 private key PEM)
    const TEST_RSA_PRIVATE_KEY_PEM: &str = "\
-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQCIjz8B7+pqqH+s
nCUbTeL6w0HhGWh6g1LCN/S17wssScQEKlaTGrsefgp9eI7zDxyEZhzObGsLZvJs
xjh8/14b4+48V9Ml3i3O/G/osh2xvaxR02EIM4WbN7x48RmZ1+l2jdghHXt85GPg
AIeW4BESUH9+I9/MARC4FcP5b73vM7kSW1HUhf9Mz5BWmmFUEP6+9T+XmvTbnXRO
i3GKHjKIjRPVjTbzaiCZH41mITbn+DVkEuQgMG5dTnaETbIqw7VUd/uLFuTGjM0K
3jld/N816z2sf2zCDj6xlYn2f1tBxiYzRSG7Y7lcnYA0CyF2rduFCOYgnJP3egJR
ulqwOYkRAgMBAAECggEAC/mKuAXkp5ZvvrBBET5A1oY3gVCP8LlSESGkwezsrQUp
bQeU6KimCrAZBaRkHaSANsx4/3FyqBDulnMB2lUu2JGBzzUQ2Q/c8rsAabZvw+mq
4hCAAF77Ou+V5YGX4f2U1X5t+sZp8Rtadja5rRVoLdPVADfPXMVlpMzUzutZa4+b
4je9fxtyQg3Of3j0qIdFX1F6/4PsEdZ5q57NH6ALnc0fEqy79kFv+zcK7Fa3CBUC
eIrxpbsrlmF08fHwi6WXP3QqZyJSLKZOZbQlDO4oEJeC2sB9FB/ZQ12qTJZwM42g
IKW5iSbdOXEvVj5zd+/v/87rX8G9pYmSwF9/9+lr+QKBgQC9hZrwsJLdOcIV2ekA
msKcNHkjFT+TnOw8SaRRD8QGTkVK5uRcxlSdwkr2fRg/hxB2hhppcvE7guCqJEiS
T4/6409ui0Z9CLjEhgdyr7qNkA+wsmkSGxu+Zv8+GYq9skYwSJPEV1Uu9QSSE3N3
RXSQlzZz34OuZkhCoqmWDRLcCQKBgQC4dc90S+HVP9XMngx5WlfLStc9AAZN8Hkf
vMMOWdwNa3n0beh5d6FuNSZbyr5B5XBSM1GikL7ernAT9CKGcoHn0jvDFNdHh9Ew
/nhFjL7nLXUqCi5Tzy5ah3OBEUrBJFou49hLnfAFY1HHItWyXDyynY3K+fUz+lis
tD/dpfIWyQKBgGsrGMFP58xnM8PtdB9eY/u2hGV9R3UuQDubHOqlsqAqNG61f56i
nAiVbJRTipmpw3pyPI8yawzO5kHvwIXTrcQeM7V71kEv5GNksuN8UU5pjyXIzTdq
0tZpIZ45DUZVf/EfqUdWZxnlfU8o5pskUFTO3QDK/Ihq2COuHZ13CRoZAoGBAIlq
wsTZrwEF0EniSFqzcgox1A1OkmPHzQRWxF1RljytH6p3oqOy+qE2mT/y1zASNE4Y
iy13dapA+5/x1TKh5aMFHJ5lTUetp6s/N+xgQOvKEqnh8cdf5iFtHSA++JjQcxrR
hJY4r9Hjvs2CZv679j/+Xd6jvgcd7qeilJ2T/bj5AoGAaTtQ8d0wTs+ty5GnhjI4
Ga4yGQBr+NhnMmJWzwhhYc0iTdmmQcb4rlRkYCYtW3eeMl7gEJRMqvKz0CcEoCgY
I+Oo6yTIlciMS3T88tdHYaqaxUkTpAj37nYbGg2YwyPqJvymdC9aD/ocGSOm1Lu9
BpziQcYc5VtOHc9QJKFIn+g=
-----END PRIVATE KEY-----";

    // Base64-encoded SPKI public key matching the private key above.
    // This is the format Keycloak returns in the realm info `public_key` field.
    const TEST_KEYCLOAK_PUBLIC_KEY: &str = "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMII\
BCgKCAQEAiI8/Ae/qaqh/rJwlG03i+sNB4RloeoNSwjf0te8LLEnEBCpWkxq7Hn4KfXiO8w8chG\
Yczmxr C2bybMY4fP9eG+PuPFfTJd4tzvxv6LIdsb2sUdNhCDOFmze8ePEZmdfpdo3YIR17fORj4\
ACHluARElB/fiPfzAEQuBXD+W+97zO5EltR1IX/TM+QVpphVBD+vvU/l5r02510Totxih4yiI0T1\
Y0282ogmR+NZiE25/g1ZBLkIDBuXU52hE2yKsO1VHf7ixbkxozNCt45XfzfNes9rH9swg4+sZWJ\
9n9bQcYmM0Uhu2O5XJ2ANAshdq3bhQjmIJyT93oCUbpasDmJEQIDAQAB";

    const TOKEN_EXPIRE_HOURS: u64 = 2;

    fn sign_test_token(claims: &TokenClaims) -> String {
        let key = EncodingKey::from_rsa_pem(TEST_RSA_PRIVATE_KEY_PEM.as_bytes())
            .expect("test private key should parse");

        let iat = get_current_timestamp();
        let exp = iat + TOKEN_EXPIRE_HOURS * 3600;

        // Wrap TokenClaims with standard JWT fields
        #[derive(Serialize)]
        struct FullClaims<'a> {
            iat: u64,
            exp: u64,
            #[serde(flatten)]
            inner: &'a TokenClaims,
        }

        let full = FullClaims {
            iat,
            exp,
            inner: claims,
        };

        encode(&Header::new(Algorithm::RS256), &full, &key).expect("encoding should succeed")
    }

    #[test]
    fn keycloak_key_format_parses_successfully() {
        let result = decoding_key_from_keycloak(TEST_KEYCLOAK_PUBLIC_KEY);
        assert!(result.is_ok(), "should parse Keycloak SPKI base64 key");
    }

    #[test]
    fn sign_and_verify_with_keycloak_key_format() {
        let claims = TokenClaims {
            roles: Some(vec!["FleetAdministrator".to_string()]),
            tenant_list: Some(vec!["cp".to_string()]),
            fleet_list: None,
        };

        let token = sign_test_token(&claims);

        // Verify using the same code path as production
        let pub_key =
            decoding_key_from_keycloak(TEST_KEYCLOAK_PUBLIC_KEY).expect("key should parse");
        let mut validation = Validation::new(Algorithm::RS256);
        validation.validate_exp = true;
        validation.validate_aud = false;
        validation.required_spec_claims.remove("iss");
        validation.required_spec_claims.remove("aud");

        let decoded = decode::<TokenClaims>(&token, &pub_key, &validation);
        assert!(decoded.is_ok(), "signature verification should succeed");

        let decoded_claims = decoded.unwrap().claims;
        assert_eq!(
            decoded_claims.roles.as_deref(),
            Some(["FleetAdministrator".to_string()].as_slice())
        );
        assert_eq!(
            decoded_claims.tenant_list.as_deref(),
            Some(["cp".to_string()].as_slice())
        );
    }

    #[test]
    fn invalid_key_fails() {
        let result = decoding_key_from_keycloak("not-valid-base64-key!!!");
        assert!(result.is_err());
    }
}
