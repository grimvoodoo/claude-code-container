-- Tasks table
CREATE TABLE IF NOT EXISTS tasks (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    prompt      TEXT        NOT NULL,
    repo        TEXT,
    branch      TEXT,
    status      TEXT        NOT NULL DEFAULT 'pending',
    work_dir    TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Events table (append-only log for a task's output)
CREATE TABLE IF NOT EXISTS task_events (
    id          BIGSERIAL   PRIMARY KEY,
    task_id     UUID        NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    event_type  TEXT        NOT NULL,   -- 'output' | 'stderr' | 'system' | 'status' | 'input_error'
    text        TEXT,                   -- for output / stderr / system events
    status      TEXT,                   -- for status events
    exit_code   INTEGER,                -- for status events
    signal      TEXT,                   -- for status events
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS task_events_task_id_idx ON task_events (task_id);
