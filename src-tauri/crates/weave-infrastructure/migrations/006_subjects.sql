-- Referenční postavy: pojmenovaná osoba/subjekt s několika fotkami, které se
-- dají jedním klikem přiložit jako reference (PuLID) při generování obrázků.
CREATE TABLE IF NOT EXISTS subjects (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    notes      TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS subject_images (
    id         TEXT PRIMARY KEY,
    subject_id TEXT NOT NULL REFERENCES subjects(id) ON DELETE CASCADE,
    path       TEXT NOT NULL,
    mime       TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_subject_images_subject ON subject_images(subject_id);
