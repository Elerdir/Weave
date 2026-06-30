CREATE TABLE IF NOT EXISTS workspace_files (
    path        TEXT PRIMARY KEY NOT NULL,
    name        TEXT NOT NULL,
    extension   TEXT,
    size_bytes  INTEGER NOT NULL DEFAULT 0,
    modified_at TEXT NOT NULL,
    indexed_at  TEXT NOT NULL,
    text_content TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_workspace_extension ON workspace_files(extension);
CREATE INDEX IF NOT EXISTS idx_workspace_modified  ON workspace_files(modified_at DESC);

-- FTS5 full-text search přes obsah a jméno souboru
CREATE VIRTUAL TABLE IF NOT EXISTS workspace_files_fts USING fts5(
    name,
    text_content,
    content='workspace_files',
    content_rowid='rowid',
    tokenize='unicode61 remove_diacritics 2'
);

CREATE TRIGGER IF NOT EXISTS workspace_files_ai AFTER INSERT ON workspace_files BEGIN
    INSERT INTO workspace_files_fts(rowid, name, text_content)
    VALUES (new.rowid, new.name, new.text_content);
END;

CREATE TRIGGER IF NOT EXISTS workspace_files_au AFTER UPDATE ON workspace_files BEGIN
    INSERT INTO workspace_files_fts(workspace_files_fts, rowid, name, text_content)
    VALUES ('delete', old.rowid, old.name, old.text_content);
    INSERT INTO workspace_files_fts(rowid, name, text_content)
    VALUES (new.rowid, new.name, new.text_content);
END;

CREATE TRIGGER IF NOT EXISTS workspace_files_ad AFTER DELETE ON workspace_files BEGIN
    INSERT INTO workspace_files_fts(workspace_files_fts, rowid, name, text_content)
    VALUES ('delete', old.rowid, old.name, old.text_content);
END;

-- Uložení cesty aktuálního workspace (singleton)
CREATE TABLE IF NOT EXISTS workspace_config (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
