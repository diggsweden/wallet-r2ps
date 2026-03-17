-- Device state aggregate head (optimistic concurrency control)
CREATE TABLE IF NOT EXISTS device_state_head (
    device_id       TEXT        PRIMARY KEY,
    current_version BIGINT      NOT NULL,
    updated_at      TIMESTAMPTZ DEFAULT now()
);

-- Append-only event log
CREATE TABLE IF NOT EXISTS device_state_version (
    device_id       TEXT        NOT NULL,
    version         BIGINT      NOT NULL,
    state_jws       TEXT        NOT NULL,
    command_type    TEXT        NOT NULL,
    correlation_id  TEXT        NOT NULL,
    created_at      TIMESTAMPTZ DEFAULT now(),
    PRIMARY KEY (device_id, version)
);

-- Transactional outbox for reliable event publishing
CREATE TABLE IF NOT EXISTS outbox (
    id          BIGSERIAL   PRIMARY KEY,
    topic       TEXT        NOT NULL,
    key         TEXT        NOT NULL,
    payload     JSONB       NOT NULL,
    created_at  TIMESTAMPTZ DEFAULT now(),
    published   BOOLEAN     DEFAULT false
);

CREATE INDEX IF NOT EXISTS idx_outbox_unpublished ON outbox(id) WHERE NOT published;

-- Notify the outbox relay when new entries are inserted
CREATE OR REPLACE FUNCTION notify_outbox_insert()
RETURNS trigger AS $$
BEGIN
    PERFORM pg_notify('outbox_channel', '');
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS outbox_after_insert ON outbox;
CREATE TRIGGER outbox_after_insert
    AFTER INSERT ON outbox
    FOR EACH STATEMENT
    EXECUTE FUNCTION notify_outbox_insert();
