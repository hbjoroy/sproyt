-- Pipe into a restricted local file with psql -X -qAt -v ON_ERROR_STOP=1.
-- jsonb aggregates are taken in the same statement snapshot as coverage.
select jsonb_build_object(
    'version', 1,
    'lineage', c.lineage,
    'covered_from', c.covered_from,
    'captured_at', statement_timestamp(),
    'obligations', coalesce((
        select jsonb_agg(jsonb_build_object(
            'id', o.id, 'recorded_at', o.recorded_at, 'account_id', o.account_id,
            'action', o.action, 'native_family', o.native_family
        ) order by o.id) from privacy_journal.obligations o
    ), '[]'::jsonb)
) from privacy_journal.coverage c;
