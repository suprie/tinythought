use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordVerifier};

const SESSION_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);
const LOGIN_WINDOW: Duration = Duration::from_secs(5 * 60);
const MAX_FAILED_LOGINS: usize = 10;

struct Session {
    csrf_token: String,
    expires_at: Instant,
}

pub struct AdminAuth {
    username: String,
    password_hash: String,
    secure_cookie: bool,
    sessions: Mutex<HashMap<String, Session>>,
    failed_logins: Mutex<VecDeque<Instant>>,
}

impl AdminAuth {
    pub fn new(
        username: String,
        password_hash: String,
        secure_cookie: bool,
    ) -> anyhow::Result<Self> {
        PasswordHash::new(&password_hash)
            .map_err(|error| anyhow::anyhow!("invalid ADMIN_PASSWORD_HASH: {error}"))?;
        if username.is_empty() {
            anyhow::bail!("ADMIN_USERNAME must not be empty");
        }
        Ok(Self {
            username,
            password_hash,
            secure_cookie,
            sessions: Mutex::new(HashMap::new()),
            failed_logins: Mutex::new(VecDeque::new()),
        })
    }

    pub async fn verify_credentials(&self, username: &str, password: &str) -> bool {
        {
            let mut failures = self.failed_logins.lock().unwrap();
            let now = Instant::now();
            while failures
                .front()
                .is_some_and(|at| now.duration_since(*at) >= LOGIN_WINDOW)
            {
                failures.pop_front();
            }
            if failures.len() >= MAX_FAILED_LOGINS {
                return false;
            }
        }

        let hash = self.password_hash.clone();
        let password = password.to_owned();
        let password_matches = tokio::task::spawn_blocking(move || {
            PasswordHash::new(&hash).ok().is_some_and(|parsed| {
                Argon2::default()
                    .verify_password(password.as_bytes(), &parsed)
                    .is_ok()
            })
        })
        .await
        .unwrap_or(false);
        let valid = username == self.username && password_matches;
        if valid {
            self.failed_logins.lock().unwrap().clear();
        } else {
            self.failed_logins.lock().unwrap().push_back(Instant::now());
        }
        valid
    }

    pub fn create_session(&self) -> (String, String) {
        let session_id = random_hex();
        let csrf_token = random_hex();
        let mut sessions = self.sessions.lock().unwrap();
        sessions.retain(|_, session| session.expires_at > Instant::now());
        sessions.insert(
            session_id.clone(),
            Session {
                csrf_token: csrf_token.clone(),
                expires_at: Instant::now() + SESSION_LIFETIME,
            },
        );
        (session_id, csrf_token)
    }

    pub fn session_csrf(&self, session_id: &str) -> Option<String> {
        let mut sessions = self.sessions.lock().unwrap();
        if sessions.get(session_id)?.expires_at <= Instant::now() {
            sessions.remove(session_id);
            return None;
        }
        sessions
            .get(session_id)
            .map(|session| session.csrf_token.clone())
    }

    pub fn revoke_session(&self, session_id: &str) {
        self.sessions.lock().unwrap().remove(session_id);
    }

    pub fn cookie_name(&self) -> &'static str {
        if self.secure_cookie {
            "__Host-tt_admin"
        } else {
            "tt_admin"
        }
    }

    pub fn cookie(&self, session_id: &str) -> String {
        let secure = if self.secure_cookie { "; Secure" } else { "" };
        format!(
            "{}={session_id}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}{}",
            self.cookie_name(),
            SESSION_LIFETIME.as_secs(),
            secure
        )
    }

    pub fn expired_cookie(&self) -> String {
        let secure = if self.secure_cookie { "; Secure" } else { "" };
        format!(
            "{}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{}",
            self.cookie_name(),
            secure
        )
    }
}

fn random_hex() -> String {
    rand::random::<[u8; 32]>()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use argon2::password_hash::{PasswordHasher, SaltString};

    #[tokio::test]
    async fn credentials_and_sessions_are_separate() {
        let salt = SaltString::encode_b64(&[7; 16]).unwrap();
        let hash = Argon2::default()
            .hash_password(b"a strong password", &salt)
            .unwrap()
            .to_string();
        let auth = AdminAuth::new("owner".to_string(), hash, true).unwrap();

        assert!(
            !auth
                .verify_credentials("someone", "a strong password")
                .await
        );
        assert!(!auth.verify_credentials("owner", "wrong password").await);
        assert!(auth.verify_credentials("owner", "a strong password").await);

        let (session_id, csrf) = auth.create_session();
        assert_ne!(session_id, "a strong password");
        assert_eq!(auth.session_csrf(&session_id), Some(csrf));
        assert!(auth.cookie(&session_id).contains("; Secure"));
        assert!(auth.cookie(&session_id).contains("; HttpOnly"));

        auth.revoke_session(&session_id);
        assert_eq!(auth.session_csrf(&session_id), None);
    }
}
