use crate::domain::{
    ChannelId, ChannelSlug, DisplayName, RepositoryError, ResolvedChannelReference,
};

pub(super) type ChannelReferenceRow =
    (Option<String>, Option<String>, Option<String>, Option<bool>);

pub(super) fn statement(pg: bool) -> String {
    let tokens = if pg {
        "jsonb_array_elements_text(?::jsonb)"
    } else {
        "json_each(?)"
    };
    crate::chatbot::sql(
        &format!(
            "with source as (
                select c.circle_id,m.user_id
                from channel_memberships m join channels c on c.id=m.channel_id
                where m.user_id=?uuid and c.id=?uuid
                  and (c.circle_id is null or exists(
                    select 1 from circle_memberships cm
                    where cm.circle_id=c.circle_id and cm.user_id=m.user_id
                  ))
             ), requested as (
                select distinct value as slug from {tokens}
             ), matched as (
                select requested.slug,cast(target.id as text) as id,target.name,
                       exists(select 1 from channel_memberships tm
                         where tm.channel_id=target.id and tm.user_id=source.user_id) as is_member,
                       count(*) over (partition by requested.slug) as choices
                from source cross join requested
                join channels target on (
                    target.slug=requested.slug
                    or target.slug=replace(cast(source.circle_id as text),'-','') || '-' || requested.slug
                )
                where (target.circle_id=source.circle_id or (target.circle_id is null and source.circle_id is null))
                  and target.kind in ('public','local')
             )
             select matched.id,matched.slug,matched.name,matched.is_member
             from source left join matched on matched.choices=1
             order by matched.slug"
        ),
        pg,
    )
}

pub(super) fn decode(
    rows: Vec<ChannelReferenceRow>,
) -> Result<Vec<ResolvedChannelReference>, RepositoryError> {
    if rows.is_empty() {
        return Err(RepositoryError::PermissionDenied);
    }
    rows.into_iter()
        .filter_map(|(id, slug, name, is_member)| Some((id?, slug?, name?, is_member?)))
        .map(|(id, slug, name, is_member)| {
            Ok(ResolvedChannelReference {
                id: ChannelId::new(id).map_err(super::storage)?,
                slug: ChannelSlug::new(slug).map_err(super::storage)?,
                name: DisplayName::new(name).map_err(super::storage)?,
                is_member,
            })
        })
        .collect()
}
