-- Additive for the concurrently running production binary.
ALTER TABLE channels ADD COLUMN hidden BOOLEAN NOT NULL DEFAULT FALSE;
