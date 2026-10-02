-- Pin each accepted case to its immutable Heart definition.
alter table work_items add column definition_version text not null default '1.0.0'
  check (definition_version in ('1.0.0','1.1.0'));
alter table work_item_tasks add column decision_note text
  check (length(decision_note) <= 8000);
alter table work_item_tasks add column decision_revision integer;
