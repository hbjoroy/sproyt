-- Opt-in canary native sessions. Credential/receipt payloads are AES-GCM sealed
-- with the existing session key ring and purpose/instance binding in the service.
create table native_service (instance_id text primary key, origin text not null unique, issuer text not null, schema_version text not null);
create table native_epochs (user_id text primary key, epoch text not null, changed_at text not null);
create table native_attempts (
 id text primary key, state_hash text not null unique, challenge text not null,
 expected_user_id text not null, start_epoch text not null, epoch text not null,
 user_id text not null, callback_port text not null, created_at text not null,
 expires_at text not null, phase text not null,
 cookie_hash text not null, confirm_hash text not null, code_hash text not null,
 handoff_expires text not null, redemption_id text not null, receipt_expires text not null,
 family text not null, secret text not null, receipt text not null
);
create index native_attempts_expiry on native_attempts(phase,expires_at);
create table native_families (
 family text primary key, user_id text not null, epoch text not null, generation text not null,
 phase text not null, access_hash text not null unique, refresh_hash text not null unique,
 revoke_hash text not null unique, expires_at text not null, secret text not null,
 lease_id text not null, lease_expires text not null
);
create index native_families_user on native_families(user_id,phase);
create table native_refresh_receipts (
 old_hash text not null, operation_id text not null, generation text not null,
 family text not null references native_families(family), expires_at text not null,
 result text not null, primary key(old_hash,operation_id,generation)
);
create index native_receipts_expiry on native_refresh_receipts(expires_at);
