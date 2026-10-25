use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

pub type Result<T> = std::result::Result<T, ()>;
pub const ENVIRONMENT: &str = "n01-lab";
pub const DESTINATION: &str = "windows-loopback";

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |v| v.as_secs() as i64)
}

pub fn random() -> Result<String> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| ())?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

pub fn digest(value: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(value.as_bytes()))
}

pub fn valid_challenge(value: &str) -> bool {
    value.len() == 43
        && URL_SAFE_NO_PAD
            .decode(value)
            .is_ok_and(|bytes| bytes.len() == 32 && URL_SAFE_NO_PAD.encode(bytes) == value)
}

pub fn valid_verifier(value: &str) -> bool {
    (43..=128).contains(&value.len())
        && value
            .bytes()
            .all(|v| v.is_ascii_alphanumeric() || b"-._~".contains(&v))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Start {
    pub challenge: String,
    pub platform: String,
    pub environment: String,
    pub destination_id: String,
    pub expected_user_id: Option<String>,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Exchange {
    pub transaction_id: String,
    pub code: String,
    pub verifier: String,
    pub redemption_id: String,
    pub environment: String,
    pub destination_id: String,
}

// Deliberately no Debug: fixtures still must not print credentials or proofs.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct Receipt {
    pub kind: String,
    pub user_id: String,
    pub session_family: String,
    pub credential: String,
}

pub struct BrowserAttempt {
    pub id: String,
    pub state: String,
    pub nonce: String,
    pub verifier: String,
    pub cookie: String,
}

pub struct CallbackAttempt {
    pub id: String,
    pub nonce: String,
    pub verifier: String,
}

pub struct Store {
    connection: Connection,
    login_seconds: i64,
}

type RedemptionRow = (
    String,
    String,
    String,
    String,
    i64,
    Option<String>,
    Option<String>,
    Option<i64>,
);

impl Store {
    pub fn create(path: &Path, interactive: bool) -> Result<Self> {
        // Claim a fresh fixture file; never initialize someone else's database.
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|_| ())?;
        let mut store = Self::initialize(Connection::open(path).map_err(|_| ())?)?;
        store.login_seconds = if interactive { 600 } else { 120 };
        Ok(store)
    }

    fn initialize(connection: Connection) -> Result<Self> {
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE attempts (
              id TEXT PRIMARY KEY, challenge TEXT NOT NULL, expected_user_id TEXT,
              environment TEXT NOT NULL, destination TEXT NOT NULL, expires INTEGER NOT NULL,
              phase TEXT NOT NULL, oidc_state TEXT UNIQUE, nonce TEXT, verifier TEXT, cookie_hash TEXT,
              user_id TEXT, code_hash TEXT, code_expires INTEGER,
              redemption_id TEXT, receipt TEXT, receipt_expires INTEGER
            );").map_err(|_| ())?;
        Ok(Self {
            connection,
            login_seconds: 120,
        })
    }

    pub fn start(&mut self, request: &Start) -> Result<String> {
        if !valid_challenge(&request.challenge)
            || request.platform != "windows"
            || request.environment != ENVIRONMENT
            || request.destination_id != DESTINATION
            || request
                .expected_user_id
                .as_ref()
                .is_some_and(|v| sproyt_protocol::UserId::new(v.clone()).is_err())
        {
            return Err(());
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| ())?;
        let count: i64 = tx
            .query_row("SELECT count(*) FROM attempts", [], |r| r.get(0))
            .map_err(|_| ())?;
        if count >= 32 {
            return Err(());
        }
        let id = random()?;
        tx.execute("INSERT INTO attempts(id,challenge,expected_user_id,environment,destination,expires,phase) VALUES(?1,?2,?3,?4,?5,?6,'new')",
            params![id, request.challenge, request.expected_user_id, ENVIRONMENT, DESTINATION, now()+self.login_seconds]).map_err(|_| ())?;
        tx.commit().map_err(|_| ())?;
        Ok(id)
    }

    pub fn begin_browser(&mut self, id: &str) -> Result<BrowserAttempt> {
        if !valid_challenge(id) {
            return Err(());
        }
        let attempt = BrowserAttempt {
            id: id.into(),
            state: random()?,
            nonce: random()?,
            verifier: random()?,
            cookie: random()?,
        };
        let changed = self.connection.execute("UPDATE attempts SET phase='authorizing',oidc_state=?1,nonce=?2,verifier=?3,cookie_hash=?4 WHERE id=?5 AND phase='new' AND expires>?6",
            params![attempt.state, attempt.nonce, attempt.verifier, digest(&attempt.cookie), id, now()]).map_err(|_| ())?;
        if changed != 1 {
            return Err(());
        }
        Ok(attempt)
    }

    pub fn claim_callback(&mut self, state: &str, cookies: &str) -> Result<CallbackAttempt> {
        if !valid_challenge(state) {
            return Err(());
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| ())?;
        let (id, nonce, verifier, cookie_hash): (String,String,String,String) = tx.query_row(
            "SELECT id,nonce,verifier,cookie_hash FROM attempts WHERE oidc_state=?1 AND phase='authorizing' AND expires>?2",
            params![state, now()], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(|_| ())?.ok_or(())?;
        let name = format!("probe_{id}");
        let mut values = cookies
            .split(';')
            .filter_map(|v| v.trim().split_once('='))
            .filter(|(key, _)| *key == name)
            .map(|(_, value)| value);
        let cookie = values.next().ok_or(())?;
        if values.next().is_some() || digest(cookie) != cookie_hash {
            return Err(());
        }
        tx.execute("UPDATE attempts SET phase='verifying' WHERE id=?1", [&id])
            .map_err(|_| ())?;
        tx.commit().map_err(|_| ())?;
        Ok(CallbackAttempt {
            id,
            nonce,
            verifier,
        })
    }

    pub fn complete_callback(&mut self, id: &str, user_id: &str) -> Result<String> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| ())?;
        let (expected, expires): (Option<String>,i64) = tx.query_row(
            "SELECT expected_user_id,expires FROM attempts WHERE id=?1 AND phase='verifying' AND expires>?2", params![id,now()],
            |r| Ok((r.get(0)?,r.get(1)?))).optional().map_err(|_| ())?.ok_or(())?;
        if expected.as_deref().is_some_and(|v| v != user_id) {
            return Err(());
        }
        let code = random()?;
        tx.execute("UPDATE attempts SET phase='ready',user_id=?1,code_hash=?2,code_expires=?3,verifier=NULL,nonce=NULL,cookie_hash=NULL WHERE id=?4",
            params![user_id, digest(&code), (now()+60).min(expires), id]).map_err(|_| ())?;
        tx.commit().map_err(|_| ())?;
        Ok(code)
    }

    pub fn release_callback(&mut self, id: &str) -> Result<()> {
        self.connection
            .execute(
                "UPDATE attempts SET phase='authorizing' WHERE id=?1 AND phase='verifying'",
                [id],
            )
            .map_err(|_| ())?;
        Ok(())
    }

    pub fn exchange(&mut self, request: &Exchange) -> Result<Receipt> {
        if !valid_challenge(&request.transaction_id)
            || !valid_challenge(&request.code)
            || !valid_verifier(&request.verifier)
            || request.redemption_id.len() != 32
            || !request.redemption_id.bytes().all(|v| v.is_ascii_hexdigit())
            || request.environment != ENVIRONMENT
            || request.destination_id != DESTINATION
        {
            return Err(());
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| ())?;
        let (phase, challenge, user, code_hash, code_expires, redeemed, saved, receipt_expires):
            RedemptionRow = tx.query_row(
            "SELECT phase,challenge,user_id,code_hash,code_expires,redemption_id,receipt,receipt_expires FROM attempts WHERE id=?1 AND environment=?2 AND destination=?3 AND phase IN ('ready','redeemed')",
            params![request.transaction_id, request.environment, request.destination_id],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?)))
            .optional().map_err(|_| ())?.ok_or(())?;
        if digest(&request.verifier) != challenge || digest(&request.code) != code_hash {
            return Err(());
        }
        if phase == "redeemed" {
            if redeemed.as_deref() != Some(&request.redemption_id)
                || receipt_expires.is_none_or(|v| v <= now())
            {
                return Err(());
            }
            return serde_json::from_str(saved.as_deref().ok_or(())?).map_err(|_| ());
        }
        if code_expires <= now() {
            return Err(());
        }
        let receipt = Receipt {
            kind: "non_api_fixture".into(),
            user_id: user,
            session_family: random()?,
            credential: format!("fixture-only.{}", random()?),
        };
        let json = serde_json::to_string(&receipt).map_err(|_| ())?;
        tx.execute("UPDATE attempts SET phase='redeemed',redemption_id=?1,receipt=?2,receipt_expires=?3 WHERE id=?4",
            params![request.redemption_id,json,now()+60,request.transaction_id]).map_err(|_| ())?;
        tx.commit().map_err(|_| ())?;
        Ok(receipt)
    }

    pub fn expire(&mut self, id: &str, scope: &str) -> Result<()> {
        let sql = match scope {
            "login" => "UPDATE attempts SET expires=0 WHERE id=?1",
            "handoff" => "UPDATE attempts SET code_expires=0 WHERE id=?1",
            "receipt" => "UPDATE attempts SET receipt_expires=0 WHERE id=?1",
            _ => return Err(()),
        };
        if self.connection.execute(sql, [id]).map_err(|_| ())? != 1 {
            return Err(());
        }
        Ok(())
    }

    pub fn status(&self) -> Result<(i64, i64)> {
        self.connection
            .query_row("SELECT count(*),count(receipt) FROM attempts", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc7636_s256_vector_and_bounds() {
        assert_eq!(
            digest("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert!(valid_verifier(&"a".repeat(43)));
        assert!(!valid_verifier(&"a".repeat(129)));
        assert!(!valid_challenge(&"a".repeat(44)));
    }

    fn ready(store: &mut Store) -> (Exchange, String) {
        let verifier = random().unwrap();
        let id = store
            .start(&Start {
                challenge: digest(&verifier),
                platform: "windows".into(),
                environment: ENVIRONMENT.into(),
                destination_id: DESTINATION.into(),
                expected_user_id: None,
            })
            .unwrap();
        let browser = store.begin_browser(&id).unwrap();
        store
            .claim_callback(&browser.state, &format!("probe_{id}={}", browser.cookie))
            .unwrap();
        let user =
            sproyt_protocol::UserId::from_oidc("https://fixture.invalid/", "alice").to_string();
        let code = store.complete_callback(&id, &user).unwrap();
        (
            Exchange {
                transaction_id: id,
                code,
                verifier,
                redemption_id: "1".repeat(32),
                environment: ENVIRONMENT.into(),
                destination_id: DESTINATION.into(),
            },
            user,
        )
    }

    #[test]
    fn atomic_redemption_binding_retry_and_expiry() {
        let mut store = Store::initialize(Connection::open_in_memory().unwrap()).unwrap();
        let (request, user) = ready(&mut store);
        let mut wrong = request.clone();
        wrong.verifier = random().unwrap();
        assert!(store.exchange(&wrong).is_err());
        assert_eq!(store.status().unwrap().1, 0);
        let first = store.exchange(&request).unwrap();
        assert_eq!(first.user_id, user);
        assert!(first == store.exchange(&request).unwrap());
        wrong = request.clone();
        wrong.redemption_id = "2".repeat(32);
        assert!(store.exchange(&wrong).is_err());
        assert_eq!(store.status().unwrap().1, 1);
        store.expire(&request.transaction_id, "receipt").unwrap();
        assert!(store.exchange(&request).is_err());
    }
}
