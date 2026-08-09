alter table channels add column description text not null default ''
  check (length(description) <= 2000);
