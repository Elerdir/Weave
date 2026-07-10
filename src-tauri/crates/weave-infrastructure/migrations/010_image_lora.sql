-- Per-konverzační výběr LoRA pro generování obrázků.
-- NULL = automatické vyhledání na CivitAI podle promptu.
ALTER TABLE conversation_settings ADD COLUMN image_lora TEXT;
