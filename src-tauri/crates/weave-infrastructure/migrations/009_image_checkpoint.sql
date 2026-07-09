-- Per-konverzační výběr checkpointu pro generování obrázků.
-- NULL = automatická volba podle stylu promptu.
ALTER TABLE conversation_settings ADD COLUMN image_checkpoint TEXT;
