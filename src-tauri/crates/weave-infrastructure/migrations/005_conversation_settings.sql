-- Parametry generování specifické pro konverzaci (posuvníky v chatu).
-- NULL = použij globální výchozí hodnotu.
CREATE TABLE IF NOT EXISTS conversation_settings (
    conversation_id TEXT PRIMARY KEY REFERENCES conversations(id) ON DELETE CASCADE,
    context_length INTEGER,
    temperature REAL,
    max_tokens INTEGER
);
