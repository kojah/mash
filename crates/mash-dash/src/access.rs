//! Sign-in for a dashboard reached from other devices. The operator names an
//! environment variable holding an access token; a browser exchanges that
//! token once for a cookie, and every history route then requires it.
use crate::AppError;
use axum::http::{HeaderMap, header};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const COOKIE: &str = "mash_dash";
/// Wrong tokens accepted per window before sign-in answers "too many".
const ATTEMPTS: u32 = 10;
const WINDOW: Duration = Duration::from_secs(60);
const COOKIE_SECONDS: u64 = 30 * 24 * 60 * 60;
/// How long a one-time code can be used after it is issued.
const CODE_LIFETIME: Duration = Duration::from_mins(10);
/// Letters and digits that are not mistaken for one another when read aloud
/// or typed on a phone.
const CODE_ALPHABET: &[u8; 32] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";
const CODE_LENGTH: usize = 6;

pub(super) struct Access {
    credentials: Option<Credentials>,
    failures: Mutex<(Instant, u32)>,
    /// The one code a person may type instead of the token, and its expiry.
    code: Mutex<Option<(String, Instant)>>,
}

struct Credentials {
    token: blake3::Hash,
    /// What a signed-in browser presents: derived from the token, so it
    /// survives a restart and stops working when the token changes.
    session: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Refusal {
    Wrong,
    TooMany,
}

impl Access {
    /// `variable` names the environment variable holding the token; without
    /// one the dashboard asks for no sign-in.
    pub(super) fn load(variable: Option<&str>) -> Result<Self, AppError> {
        let token = variable.map(super::services::token).transpose()?;
        Ok(Self::new(token.as_deref()))
    }

    pub(super) fn new(token: Option<&str>) -> Self {
        Self {
            credentials: token.map(|token| Credentials {
                token: blake3::hash(token.as_bytes()),
                session: blake3::derive_key("mash dash session v1", token.as_bytes())
                    .iter()
                    .fold(String::new(), |mut hex, byte| {
                        use std::fmt::Write;
                        let _ = write!(hex, "{byte:02x}");
                        hex
                    }),
            }),
            failures: Mutex::new((Instant::now(), 0)),
            code: Mutex::new(None),
        }
    }

    /// A short code that signs one browser in, once, within ten minutes. It
    /// replaces any earlier code. Only a signed-in caller may ask for one.
    pub(super) fn issue_code(&self) -> Result<String, AppError> {
        let mut random = [0_u8; CODE_LENGTH];
        getrandom::fill(&mut random).map_err(|_| AppError::internal("create sign-in code"))?;
        // 32 symbols divide 256 evenly, so every symbol is equally likely.
        let code: String = random
            .iter()
            .map(|byte| char::from(CODE_ALPHABET[usize::from(byte % 32)]))
            .collect();
        *self
            .code
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some((code.clone(), Instant::now() + CODE_LIFETIME));
        Ok(code)
    }

    /// Whether `typed` is the live code, however it was spaced or cased. A
    /// match uses the code up.
    fn redeem(&self, typed: &str) -> bool {
        let typed: String = typed
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .map(|c| c.to_ascii_uppercase())
            .collect();
        let mut code = self
            .code
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let matched = code.as_ref().is_some_and(|(expected, expires)| {
            Instant::now() < *expires
                && blake3::hash(typed.as_bytes()) == blake3::hash(expected.as_bytes())
        });
        if matched {
            *code = None;
        }
        matched
    }

    pub(super) fn required(&self) -> bool {
        self.credentials.is_some()
    }

    /// Whether this request carries the session cookie or the token itself.
    pub(super) fn signed_in(&self, headers: &HeaderMap) -> bool {
        let Some(credentials) = &self.credentials else {
            return true;
        };
        let bearer = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .is_some_and(|token| blake3::hash(token.as_bytes()) == credentials.token);
        bearer
            || headers
                .get_all(header::COOKIE)
                .iter()
                .filter_map(|value| value.to_str().ok())
                .flat_map(|value| value.split(';'))
                .filter_map(|pair| pair.trim().split_once('='))
                .any(|(name, value)| {
                    // Hashing both sides makes the comparison constant-time.
                    name == COOKIE
                        && blake3::hash(value.as_bytes())
                            == blake3::hash(credentials.session.as_bytes())
                })
    }

    /// The `Set-Cookie` value for a correct token or one-time code. Wrong ones
    /// are counted so neither can be guessed at network speed.
    pub(super) fn sign_in(&self, token: &str, secure: bool) -> Result<String, Refusal> {
        let Some(credentials) = &self.credentials else {
            return Err(Refusal::Wrong);
        };
        let mut failures = self
            .failures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if failures.0.elapsed() >= WINDOW {
            *failures = (Instant::now(), 0);
        }
        if failures.1 >= ATTEMPTS {
            return Err(Refusal::TooMany);
        }
        if blake3::hash(token.as_bytes()) != credentials.token && !self.redeem(token) {
            failures.1 += 1;
            return Err(Refusal::Wrong);
        }
        Ok(cookie(&credentials.session, COOKIE_SECONDS, secure))
    }

    /// The `Set-Cookie` value that makes a browser forget its session.
    pub(super) fn sign_out(secure: bool) -> String {
        cookie("", 0, secure)
    }
}

fn cookie(value: &str, seconds: u64, secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!("{COOKIE}={value}; Path=/; Max-Age={seconds}; HttpOnly; SameSite=Strict{secure}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn headers(name: header::HeaderName, value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(name, value.parse().unwrap());
        headers
    }

    #[test]
    fn a_dashboard_without_a_token_asks_for_no_sign_in() {
        let access = Access::new(None);
        assert!(!access.required());
        assert!(access.signed_in(&HeaderMap::new()));
        assert_eq!(access.sign_in(TOKEN, false), Err(Refusal::Wrong));
    }

    #[test]
    fn the_token_buys_a_cookie_that_only_that_token_makes_valid() {
        let access = Access::new(Some(TOKEN));
        assert!(access.required());
        assert!(!access.signed_in(&HeaderMap::new()));
        assert!(access.signed_in(&headers(header::AUTHORIZATION, &format!("Bearer {TOKEN}"))));
        assert!(!access.signed_in(&headers(header::AUTHORIZATION, "Bearer wrong")));
        let set = access.sign_in(TOKEN, true).unwrap();
        assert!(set.contains("HttpOnly") && set.contains("SameSite=Strict"));
        assert!(set.ends_with("; Secure"));
        assert!(!access.sign_in(TOKEN, false).unwrap().contains("Secure"));
        let pair = set.split(';').next().unwrap();
        assert!(!pair.contains(TOKEN), "the cookie never holds the token");
        assert!(access.signed_in(&headers(header::COOKIE, &format!("theme=dark; {pair}"))));
        assert!(!access.signed_in(&headers(header::COOKIE, "mash_dash=forged")));
        // Another token's cookie is worthless here.
        let other = Access::new(Some("ffffffffffffffffffffffffffffffff"));
        assert!(!other.signed_in(&headers(header::COOKIE, pair)));
        assert!(Access::sign_out(false).contains("Max-Age=0"));
    }

    #[test]
    fn a_one_time_code_signs_in_once_however_it_is_typed_and_then_expires() {
        let access = Access::new(Some(TOKEN));
        let code = access.issue_code().unwrap();
        assert_eq!(code.len(), CODE_LENGTH);
        assert!(code.bytes().all(|byte| CODE_ALPHABET.contains(&byte)));
        let typed = format!(" {}-{} ", code[..3].to_lowercase(), &code[3..]);
        assert!(access.sign_in(&typed, false).is_ok());
        assert_eq!(access.sign_in(&code, false), Err(Refusal::Wrong), "used up");
        // A newer code replaces an older one, and a stale one stops working.
        let first = access.issue_code().unwrap();
        let second = access.issue_code().unwrap();
        if first != second {
            assert_eq!(access.sign_in(&first, false), Err(Refusal::Wrong));
        }
        access.code.lock().unwrap().as_mut().unwrap().1 = Instant::now();
        assert_eq!(access.sign_in(&second, false), Err(Refusal::Wrong));
        // Without a token there is nothing to sign in to.
        assert_eq!(Access::new(None).sign_in(&code, false), Err(Refusal::Wrong));
    }

    #[test]
    fn wrong_tokens_are_counted_and_then_refused_even_when_right() {
        let access = Access::new(Some(TOKEN));
        for _ in 0..ATTEMPTS {
            assert_eq!(access.sign_in("wrong", false), Err(Refusal::Wrong));
        }
        assert_eq!(access.sign_in(TOKEN, false), Err(Refusal::TooMany));
        access.failures.lock().unwrap().0 = Instant::now().checked_sub(WINDOW).unwrap();
        assert!(access.sign_in(TOKEN, false).is_ok());
    }
}
