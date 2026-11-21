//! Approved, versioned identities; an agent display name never selects a face.
use std::{borrow::Cow, sync::OnceLock};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub(crate) struct Identity {
    pub id: &'static str,
    pub label: &'static str,
    pub reference: Cow<'static, [u8]>,
    pub description: &'static str,
}

static MARIA: Identity = Identity {
    id: "maria-v1",
    label: "Maria (30–40 år)",
    reference: Cow::Borrowed(include_bytes!(
        "../../assets/imagegen-characters/maria/references/00-canonical-maria.jpg"
    )),
    description: include_str!("../../assets/imagegen-characters/maria/identity.txt"),
};

const LOTTE_HASH: &str = "3d091802e39131e3663a6fb950205396f1fa866fdf9bd852c4f047d1ba5bf200";

fn lotte() -> Option<&'static Identity> {
    static LOTTE: OnceLock<Option<Identity>> = OnceLock::new();
    LOTTE
        .get_or_init(|| {
            let portrait = std::fs::read("/var/run/sproyt-identities/lotte-v1.jpg").ok()?;
            let identity = Identity {
                id: "lotte-v1",
                label: "Lotte",
                reference: Cow::Owned(portrait),
                description: include_str!("../../assets/imagegen-characters/lotte/identity.txt"),
            };
            (identity.sha256() == LOTTE_HASH).then_some(identity)
        })
        .as_ref()
}

impl Identity {
    pub fn sha256(&self) -> String {
        hex::encode(Sha256::digest(self.reference.as_ref()))
    }
}

pub(crate) fn get(id: &str) -> Option<&'static Identity> {
    match id {
        "maria-v1" => Some(&MARIA),
        "lotte-v1" => lotte(),
        _ => None,
    }
}
pub(crate) fn catalogue() -> Value {
    Value::Array(
        std::iter::once(&MARIA)
            .chain(lotte())
            .map(|identity| json!({"id":identity.id,"label":identity.label}))
            .collect(),
    )
}
