-- Vlastní persony (vestavěné jsou v kódu, tady jen uživatelské).
CREATE TABLE IF NOT EXISTS personas (
    id            TEXT PRIMARY KEY NOT NULL,
    name          TEXT NOT NULL,
    icon          TEXT NOT NULL DEFAULT '🎭',
    system_prompt TEXT NOT NULL,
    created_at    TEXT NOT NULL
);
