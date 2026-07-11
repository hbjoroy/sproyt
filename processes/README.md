# Heart process definitions

Deploy `event-planning.yaml` to the same Heart namespace configured for
Sproyt. The application supplies its durable `process_link_id` in start
metadata and uses that value for receive-node correlation. This avoids using a
display name, channel slug, or Heart-generated identifier as an authorization
boundary.

The pilot must be enabled for the circle through the `heart.event-planning`
feature flag. Removing `SPROYT_HEART_URL` is the deployment-wide kill switch;
ordinary chat remains available and pending outbox records remain durable.
