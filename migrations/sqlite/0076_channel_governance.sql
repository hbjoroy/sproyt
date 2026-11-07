-- Global navigation visibility only: memberships, messages and subscriptions survive.
ALTER TABLE channels ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0 CHECK (hidden IN (0,1));
