-- Bootstrap a dedicated journal database as its owner. Keep application and
-- restore credentials out of this database. Provision logins separately.
begin;
create role privacy_journal_writer nologin;
create role privacy_journal_reader nologin;
create schema privacy_journal;
revoke all on schema privacy_journal from public;
grant usage on schema privacy_journal to privacy_journal_writer, privacy_journal_reader;

create table privacy_journal.coverage (
    singleton boolean primary key default true check (singleton),
    lineage uuid not null,
    covered_from timestamptz not null
);
create table privacy_journal.obligations (
    id bigint generated always as identity primary key,
    recorded_at timestamptz not null default clock_timestamp(),
    account_id uuid not null,
    action text not null,
    native_family text,
    check (
        (action = 'erase_account' and native_family is null)
        or (action = 'revoke_native_family' and native_family is not null
            and native_family ~ '^[A-Za-z0-9_-]{1,128}$')
    )
);
grant insert (account_id, action, native_family) on privacy_journal.obligations to privacy_journal_writer;
grant select on privacy_journal.coverage, privacy_journal.obligations to privacy_journal_reader;
commit;
