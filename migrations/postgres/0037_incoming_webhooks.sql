create table incoming_alert_occurrences (
  agent_id uuid not null references agent_profiles(agent_id) on delete cascade,
  fingerprint text not null,
  starts_at timestamptz not null,
  state text not null check (state in ('pending', 'firing', 'resolved')),
  firing_message_id uuid null references messages(id),
  resolved_message_id uuid null references messages(id),
  updated_at timestamptz not null default now(),
  primary key (agent_id, fingerprint, starts_at)
);

create table incoming_report_deliveries (
  agent_id uuid not null references agent_profiles(agent_id) on delete cascade,
  report_id text not null,
  message_id uuid null references messages(id),
  created_at timestamptz not null default now(),
  primary key (agent_id, report_id)
);

create index incoming_alert_occurrences_updated_idx
  on incoming_alert_occurrences(updated_at);
