use crate::sessions;
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_session: Option<sessions::Grant>,
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
    session: Option<sessions::Context>,
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

    pub fn open_session_lab(
        path: &Path,
        interactive: bool,
        origin: &str,
        issuer: &str,
    ) -> Result<Self> {
        let fresh = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
        {
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
            Err(_) => return Err(()),
        };
        let mut store = if fresh {
            Self::initialize(Connection::open(path).map_err(|_| ())?)?
        } else {
            Self {
                connection: Connection::open_with_flags(
                    path,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
                )
                .map_err(|_| ())?,
                login_seconds: 120,
                session: None,
            }
        };
        store.session = Some(if fresh {
            let context = sessions::Context {
                instance_id: random()?,
                origin: origin.into(),
                issuer: issuer.into(),
            };
            sessions::initialize(&store.connection, &context)?;
            context
        } else {
            sessions::reopen(&store.connection, origin, issuer)?
        });
        store.login_seconds = if interactive { 600 } else { 120 };
        Ok(store)
    }
    pub fn instance_id(&self) -> Option<&str> {
        self.session
            .as_ref()
            .map(|context| context.instance_id.as_str())
    }
    pub fn session_probe(&mut self, token: &str) -> Result<sessions::Claims> {
        self.session.as_ref().ok_or(())?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| ())?;
        sessions::probe(&tx, token)
    }
    pub fn session_refresh(
        &mut self,
        token: &str,
        input: &sessions::Refresh,
    ) -> Result<sessions::Grant> {
        self.session.as_ref().ok_or(())?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| ())?;
        let grant = sessions::refresh(&tx, token, input)?;
        tx.commit().map_err(|_| ())?;
        Ok(grant)
    }
    pub fn session_revoke(&mut self, token: &str) -> Result<()> {
        self.session.as_ref().ok_or(())?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| ())?;
        sessions::revoke(&tx, token)?;
        tx.commit().map_err(|_| ())
    }
    pub fn session_epoch(&mut self, user: &str) -> Result<u64> {
        self.session.as_ref().ok_or(())?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| ())?;
        let epoch = sessions::bump_epoch(&tx, user)?;
        tx.commit().map_err(|_| ())?;
        Ok(epoch)
    }
    pub fn session_cancel_login(
        &mut self,
        id: &str,
        verifier: &str,
        environment: &str,
        destination: &str,
    ) -> Result<()> {
        self.session.as_ref().ok_or(())?;
        if !valid_challenge(id)
            || !valid_verifier(verifier)
            || environment != ENVIRONMENT
            || destination != DESTINATION
        {
            return Err(());
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| ())?;
        let (challenge,phase,saved):(String,String,Option<String>)=tx.query_row("SELECT challenge,phase,receipt FROM attempts WHERE id=?1 AND environment=?2 AND destination=?3",params![id,environment,destination],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional().map_err(|_| ())?.ok_or(())?;
        if digest(verifier) != challenge {
            return Err(());
        }
        if phase != "cancelled" {
            if let Some(json) = saved {
                let receipt: Receipt = serde_json::from_str(&json).map_err(|_| ())?;
                let grant = receipt.native_session.as_ref().ok_or(())?;
                // The family comes only from our committed handoff receipt, never
                // from a caller. Retain hashes/receipt so cancellation is retryable.
                if receipt.session_family != grant.family {
                    return Err(());
                }
                tx.execute(
                    "UPDATE session_families SET revoked=1 WHERE family=?1",
                    [&grant.family],
                )
                .map_err(|_| ())?;
            }
            tx.execute("UPDATE attempts SET phase='cancelled',verifier=NULL,nonce=NULL,cookie_hash=NULL WHERE id=?1",[id]).map_err(|_| ())?;
        }
        tx.commit().map_err(|_| ())
    }

    pub fn session_expire(&mut self, family: &str, scope: &str) -> Result<()> {
        self.session.as_ref().ok_or(())?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| ())?;
        sessions::expire(&tx, family, scope)?;
        tx.commit().map_err(|_| ())
    }

    fn initialize(connection: Connection) -> Result<Self> {
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE attempts (
              id TEXT PRIMARY KEY, challenge TEXT NOT NULL, expected_user_id TEXT,
              environment TEXT NOT NULL, destination TEXT NOT NULL, expires INTEGER NOT NULL,
              phase TEXT NOT NULL, oidc_state TEXT UNIQUE, nonce TEXT, verifier TEXT, cookie_hash TEXT,
              user_id TEXT, code_hash TEXT, code_expires INTEGER,
              redemption_id TEXT, receipt TEXT, receipt_expires INTEGER, start_epoch INTEGER
            );").map_err(|_| ())?;
        Ok(Self {
            connection,
            login_seconds: 120,
            session: None,
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
        let start_epoch = if self.session.is_some() {
            Some(sessions::epoch(
                &tx,
                request.expected_user_id.as_deref().ok_or(())?,
            )?)
        } else {
            None
        };
        let count: i64 = tx
            .query_row("SELECT count(*) FROM attempts", [], |r| r.get(0))
            .map_err(|_| ())?;
        if count >= 32 {
            return Err(());
        }
        let id = random()?;
        tx.execute("INSERT INTO attempts(id,challenge,expected_user_id,environment,destination,expires,phase,start_epoch) VALUES(?1,?2,?3,?4,?5,?6,'new',?7)",
            params![id, request.challenge, request.expected_user_id, ENVIRONMENT, DESTINATION, now()+self.login_seconds,start_epoch.map(|value|value as i64)]).map_err(|_| ())?;
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
        let (expected, expires, start_epoch): (Option<String>,i64,Option<i64>) = tx.query_row(
            "SELECT expected_user_id,expires,start_epoch FROM attempts WHERE id=?1 AND phase='verifying' AND expires>?2", params![id,now()],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(|_| ())?.ok_or(())?;
        if expected.as_deref().is_some_and(|v| v != user_id) {
            return Err(());
        }
        if self.session.is_some() && start_epoch != Some(sessions::epoch(&tx, user_id)? as i64) {
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
        if self.session.is_some() {
            let start_epoch: Option<i64> = tx
                .query_row(
                    "SELECT start_epoch FROM attempts WHERE id=?1",
                    [&request.transaction_id],
                    |row| row.get(0),
                )
                .map_err(|_| ())?;
            if start_epoch != Some(sessions::epoch(&tx, &user)? as i64) {
                return Err(());
            }
        }
        if phase == "redeemed" {
            if redeemed.as_deref() != Some(&request.redemption_id)
                || receipt_expires.is_none_or(|v| v <= now())
            {
                return Err(());
            }
            let receipt: Receipt =
                serde_json::from_str(saved.as_deref().ok_or(())?).map_err(|_| ())?;
            if self.session.is_some() {
                sessions::valid_family(&tx, receipt.native_session.as_ref().ok_or(())?)?;
            }
            return Ok(receipt);
        }
        if code_expires <= now() {
            return Err(());
        }
        let family = random()?;
        let native_session = if let Some(context) = &self.session {
            Some(sessions::issue(&tx, context, &user, family.clone())?)
        } else {
            None
        };
        let receipt = Receipt {
            kind: "non_api_fixture".into(),
            user_id: user,
            session_family: family,
            credential: format!("fixture-only.{}", random()?),
            native_session,
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

    fn session_store() -> Store {
        let mut store = Store::initialize(Connection::open_in_memory().unwrap()).unwrap();
        let context = sessions::Context {
            instance_id: random().unwrap(),
            origin: "http://127.0.0.1:19191".into(),
            issuer: "http://127.0.0.1:19191/provider".into(),
        };
        sessions::initialize(&store.connection, &context).unwrap();
        store.session = Some(context);
        store
    }
    fn session_ready(store: &mut Store) -> (Exchange, String) {
        let user = sproyt_protocol::UserId::from_oidc("http://127.0.0.1:19191/provider", "alice")
            .to_string();
        let verifier = random().unwrap();
        let id = store
            .start(&Start {
                challenge: digest(&verifier),
                platform: "windows".into(),
                environment: ENVIRONMENT.into(),
                destination_id: DESTINATION.into(),
                expected_user_id: Some(user.clone()),
            })
            .unwrap();
        let browser = store.begin_browser(&id).unwrap();
        store
            .claim_callback(&browser.state, &format!("probe_{id}={}", browser.cookie))
            .unwrap();
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
    fn session_rotation_exact_retry_revoke_and_epoch_fence_all_receipts() {
        let mut store = session_store();
        let (handoff, user) = session_ready(&mut store);
        let receipt = store.exchange(&handoff).unwrap();
        let grant = receipt.native_session.as_ref().unwrap();
        assert!(
            receipt.session_family == grant.family && grant.generation == 0 && grant.epoch == 0
        );
        assert!(store.session_probe(&grant.access_token).is_ok());
        let rotate = sessions::Refresh {
            operation_id: "a".repeat(32),
            generation: 0,
        };
        let rotated = store
            .session_refresh(&grant.refresh_token, &rotate)
            .unwrap();
        assert!(rotated.generation == 1 && rotated.revoke_token == grant.revoke_token);
        assert!(store.session_probe(&grant.access_token).is_err());
        assert!(store.session_probe(&rotated.access_token).is_ok());
        assert!(
            store
                .session_refresh(&grant.refresh_token, &rotate)
                .unwrap()
                == rotated
        );
        assert!(
            store
                .session_refresh(
                    &grant.refresh_token,
                    &sessions::Refresh {
                        operation_id: "b".repeat(32),
                        generation: 0
                    }
                )
                .is_err()
        );
        assert!(
            store
                .session_refresh(&rotated.refresh_token, &rotate)
                .is_err()
        );
        store.session_revoke(&grant.revoke_token).unwrap();
        store.session_revoke(&grant.revoke_token).unwrap();
        assert!(store.session_probe(&rotated.access_token).is_err());
        assert!(
            store
                .session_refresh(&grant.refresh_token, &rotate)
                .is_err()
        );
        assert!(store.exchange(&handoff).is_err());
        let (second, _) = session_ready(&mut store);
        let second_grant = store.exchange(&second).unwrap().native_session.unwrap();
        let second_result = store
            .session_refresh(&second_grant.refresh_token, &rotate)
            .unwrap();
        assert!(store.session_epoch(&user).unwrap() == 1);
        assert!(store.session_probe(&second_result.access_token).is_err());
        assert!(
            store
                .session_refresh(&second_grant.refresh_token, &rotate)
                .is_err()
        );
        assert!(store.exchange(&second).is_err());
        let (third, _) = session_ready(&mut store);
        assert!(
            store
                .exchange(&third)
                .unwrap()
                .native_session
                .unwrap()
                .epoch
                == 1
        );
    }
    #[test]
    fn session_expiry_and_pending_callback_fences_do_not_issue_families() {
        let mut store = session_store();
        let (handoff, user) = session_ready(&mut store);
        store.session_epoch(&user).unwrap();
        assert!(store.exchange(&handoff).is_err());
        assert!(store.status().unwrap().1 == 0);
        let verifier = random().unwrap();
        let id = store
            .start(&Start {
                challenge: digest(&verifier),
                platform: "windows".into(),
                environment: ENVIRONMENT.into(),
                destination_id: DESTINATION.into(),
                expected_user_id: Some(user.clone()),
            })
            .unwrap();
        let browser = store.begin_browser(&id).unwrap();
        store
            .claim_callback(&browser.state, &format!("probe_{id}={}", browser.cookie))
            .unwrap();
        store.session_epoch(&user).unwrap();
        assert!(store.complete_callback(&id, &user).is_err());
        let (valid, _) = session_ready(&mut store);
        let grant = store.exchange(&valid).unwrap().native_session.unwrap();
        store.session_expire(&grant.family, "access").unwrap();
        assert!(store.session_probe(&grant.access_token).is_err());
        let rotate = sessions::Refresh {
            operation_id: "c".repeat(32),
            generation: 0,
        };
        assert!(store.session_refresh(&grant.refresh_token, &rotate).is_ok());
        store.session_expire(&grant.family, "receipt").unwrap();
        assert!(
            store
                .session_refresh(&grant.refresh_token, &rotate)
                .is_err()
        );
    }
    #[test]
    fn session_database_reopen_pins_identity_and_preserves_revocation() {
        let path = std::env::temp_dir().join(format!("sproyt-n03a-{}.sqlite", random().unwrap()));
        let origin = "http://127.0.0.1:19191";
        let issuer = "http://127.0.0.1:19191/provider";
        let mut store = Store::open_session_lab(&path, false, origin, issuer).unwrap();
        let instance = store.instance_id().unwrap().to_owned();
        let (handoff, _) = session_ready(&mut store);
        let grant = store.exchange(&handoff).unwrap().native_session.unwrap();
        let rotate = sessions::Refresh {
            operation_id: "d".repeat(32),
            generation: 0,
        };
        let rotated = store
            .session_refresh(&grant.refresh_token, &rotate)
            .unwrap();
        drop(store);
        assert!(Store::create(&path, false).is_err());
        assert!(Store::open_session_lab(&path, false, "http://127.0.0.1:19193", issuer).is_err());
        assert!(
            Store::open_session_lab(&path, false, origin, "http://127.0.0.1:19191/other").is_err()
        );
        let mut store = Store::open_session_lab(&path, false, origin, issuer).unwrap();
        assert!(store.instance_id() == Some(instance.as_str()));
        assert!(
            store
                .session_refresh(&grant.refresh_token, &rotate)
                .unwrap()
                == rotated
        );
        assert!(store.session_probe(&rotated.access_token).is_ok());
        store.session_revoke(&rotated.revoke_token).unwrap();
        drop(store);
        let mut store = Store::open_session_lab(&path, false, origin, issuer).unwrap();
        assert!(store.session_probe(&rotated.access_token).is_err());
        assert!(
            store
                .session_refresh(&grant.refresh_token, &rotate)
                .is_err()
        );
        assert!(store.exchange(&handoff).is_err());
        drop(store);
        std::fs::remove_file(&path).unwrap();
    }
    #[test]
    fn session_handoff_rolls_back_family_when_receipt_commit_fails() {
        let mut store = session_store();
        let (handoff, _) = session_ready(&mut store);
        store.connection.execute_batch("CREATE TRIGGER fixture_abort BEFORE UPDATE OF receipt ON attempts BEGIN SELECT RAISE(ABORT,'fixture abort'); END;").unwrap();
        assert!(store.exchange(&handoff).is_err());
        let count: i64 = store
            .connection
            .query_row("SELECT count(*) FROM session_families", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(count == 0 && store.status().unwrap().1 == 0);
        store
            .connection
            .execute_batch("DROP TRIGGER fixture_abort;")
            .unwrap();
        assert!(store.exchange(&handoff).unwrap().native_session.is_some());
    }
    #[test]
    fn concurrent_refresh_connections_return_one_committed_rotation() {
        let path = std::env::temp_dir().join(format!(
            "sproyt-n03a-concurrent-{}.sqlite",
            random().unwrap()
        ));
        let origin = "http://127.0.0.1:19191";
        let issuer = "http://127.0.0.1:19191/provider";
        let mut store = Store::open_session_lab(&path, false, origin, issuer).unwrap();
        let (handoff, _) = session_ready(&mut store);
        let grant = store.exchange(&handoff).unwrap().native_session.unwrap();
        drop(store);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let workers: Vec<_> = (0..2)
            .map(|_| {
                let path = path.clone();
                let token = grant.refresh_token.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let mut store = Store::open_session_lab(&path, false, origin, issuer).unwrap();
                    barrier.wait();
                    store
                        .session_refresh(
                            &token,
                            &sessions::Refresh {
                                operation_id: "e".repeat(32),
                                generation: 0,
                            },
                        )
                        .unwrap()
                })
            })
            .collect();
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert!(results[0] == results[1] && results[0].generation == 1);
        let store = Store::open_session_lab(&path, false, origin, issuer).unwrap();
        let families: i64 = store
            .connection
            .query_row("SELECT count(*) FROM session_families", [], |row| {
                row.get(0)
            })
            .unwrap();
        let receipts: i64 = store
            .connection
            .query_row("SELECT count(*) FROM session_receipts", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(families == 1 && receipts == 1);
        store
            .connection
            .pragma_update(None, "user_version", 999)
            .unwrap();
        drop(store);
        assert!(Store::open_session_lab(&path, false, origin, issuer).is_err());
        std::fs::remove_file(&path).unwrap();
    }
    #[test]
    fn proof_bound_cancel_fences_all_pending_phases_and_ignores_expiry_epoch() {
        let mut store = session_store();
        let user = sproyt_protocol::UserId::from_oidc("http://127.0.0.1:19191/provider", "alice")
            .to_string();
        for phase in ["new", "authorizing", "verifying"] {
            let verifier = random().unwrap();
            let id = store
                .start(&Start {
                    challenge: digest(&verifier),
                    platform: "windows".into(),
                    environment: ENVIRONMENT.into(),
                    destination_id: DESTINATION.into(),
                    expected_user_id: Some(user.clone()),
                })
                .unwrap();
            if phase != "new" {
                let browser = store.begin_browser(&id).unwrap();
                if phase == "verifying" {
                    store
                        .claim_callback(&browser.state, &format!("probe_{id}={}", browser.cookie))
                        .unwrap();
                }
            }
            assert!(
                store
                    .session_cancel_login(&id, &random().unwrap(), ENVIRONMENT, DESTINATION)
                    .is_err()
            );
            store.expire(&id, "login").unwrap();
            store.session_epoch(&user).unwrap();
            store
                .session_cancel_login(&id, &verifier, ENVIRONMENT, DESTINATION)
                .unwrap();
            store
                .session_cancel_login(&id, &verifier, ENVIRONMENT, DESTINATION)
                .unwrap();
            assert!(store.begin_browser(&id).is_err());
            assert!(store.complete_callback(&id, &user).is_err());
            store.release_callback(&id).unwrap(); // An in-flight callback's RAII cannot reopen it.
            let actual: String = store
                .connection
                .query_row("SELECT phase FROM attempts WHERE id=?1", [&id], |row| {
                    row.get(0)
                })
                .unwrap();
            assert!(actual == "cancelled");
        }
        assert!(store.status().unwrap().1 == 0);
    }
    #[test]
    fn cancel_after_lost_handoff_result_revokes_family_and_every_saved_result() {
        let mut store = session_store();
        let (handoff, user) = session_ready(&mut store);
        let grant = store.exchange(&handoff).unwrap().native_session.unwrap(); // Commit, then caller loses reply.
        assert!(
            store
                .session_cancel_login(
                    &handoff.transaction_id,
                    &random().unwrap(),
                    ENVIRONMENT,
                    DESTINATION
                )
                .is_err()
        );
        assert!(
            store
                .session_cancel_login(
                    &handoff.transaction_id,
                    &handoff.verifier,
                    "wrong",
                    DESTINATION
                )
                .is_err()
        );
        assert!(
            store
                .session_cancel_login(
                    &handoff.transaction_id,
                    &handoff.verifier,
                    ENVIRONMENT,
                    "wrong"
                )
                .is_err()
        );
        assert!(store.session_probe(&grant.access_token).is_ok());
        let rotate = sessions::Refresh {
            operation_id: "f".repeat(32),
            generation: 0,
        };
        let rotated = store
            .session_refresh(&grant.refresh_token, &rotate)
            .unwrap();
        store.expire(&handoff.transaction_id, "receipt").unwrap();
        store.expire(&handoff.transaction_id, "login").unwrap();
        store
            .session_cancel_login(
                &handoff.transaction_id,
                &handoff.verifier,
                ENVIRONMENT,
                DESTINATION,
            )
            .unwrap();
        assert!(store.exchange(&handoff).is_err());
        assert!(store.session_probe(&rotated.access_token).is_err());
        assert!(
            store
                .session_refresh(&grant.refresh_token, &rotate)
                .is_err()
        );
        assert!(
            store
                .session_refresh(
                    &rotated.refresh_token,
                    &sessions::Refresh {
                        operation_id: "0".repeat(32),
                        generation: 1
                    }
                )
                .is_err()
        );
        store.session_epoch(&user).unwrap();
        store
            .session_cancel_login(
                &handoff.transaction_id,
                &handoff.verifier,
                ENVIRONMENT,
                DESTINATION,
            )
            .unwrap();
        let revoked: bool = store
            .connection
            .query_row(
                "SELECT revoked=1 FROM session_families WHERE family=?1",
                [&grant.family],
                |row| row.get(0),
            )
            .unwrap();
        assert!(revoked);
    }
}
