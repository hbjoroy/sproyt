//! Approved, versioned identities; an agent display name never selects a face.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub(crate) struct Identity {
    pub id: &'static str,
    pub label: &'static str,
    pub reference: &'static [u8],
    pub description: &'static str,
}

static MARIA: Identity = Identity {
    id: "maria-v1",
    label: "Maria (30–40 år)",
    reference: include_bytes!(
        "../../assets/imagegen-characters/maria/references/00-canonical-maria.jpg"
    ),
    description: include_str!("../../assets/imagegen-characters/maria/identity.txt"),
};

impl Identity {
    pub fn sha256(&self) -> String {
        format!("{:x}", Sha256::digest(self.reference))
    }
}

pub(crate) fn get(id: &str) -> Option<&'static Identity> {
    match id {
        "maria-v1" => Some(&MARIA),
        _ => None,
    }
}
pub(crate) fn catalogue() -> Value {
    json!([{"id":MARIA.id,"label":MARIA.label}])
}
