-- Obecné klíč-hodnota úložiště pro nastavení aplikace
-- (ComfyUI URL, výchozí modely, atd. — NE tokeny, ty jsou v OS keychain).
CREATE TABLE IF NOT EXISTS app_config (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
