-- Reapply one previously authorised native-family revocation, in isolation.
-- A missing family is safe. A matching family owned by anyone else aborts.
begin;
create temp table restore_revocation on commit drop as
select :'account_id'::uuid::text as account_id, :'native_family'::text as family;
do $verify$
begin
    if exists (select 1 from native_families f join restore_revocation r using (family)
               where f.user_id <> r.account_id) then
        raise exception 'recorded family does not belong to the recorded account';
    end if;
end
$verify$;

update native_families f set phase='revoked', secret='', lease_id='', lease_expires='0'
from restore_revocation r where f.family=r.family and f.user_id=r.account_id;
delete from native_refresh_receipts f using restore_revocation r where f.family=r.family;
update native_attempts a set phase='cancelled', secret='', receipt=''
from restore_revocation r where a.family=r.family and a.user_id=r.account_id;

do $verify$
begin
    if exists (select 1 from native_families f join restore_revocation r using (family)
               where f.phase <> 'revoked' or f.secret <> '')
       or exists (select 1 from native_refresh_receipts f join restore_revocation r using (family))
       or exists (select 1 from native_attempts a join restore_revocation r using (family)
                  where a.user_id=r.account_id and a.phase <> 'cancelled') then
        raise exception 'native revocation did not reach its required state';
    end if;
end
$verify$;
commit;
