-- Per-konverzační parametry věrnosti podoby u generování obrázků.
-- NULL = použij výchozí (pulid_weight 1.0, face_detailer vypnuto).
ALTER TABLE conversation_settings ADD COLUMN pulid_weight REAL;
ALTER TABLE conversation_settings ADD COLUMN face_detailer INTEGER;
