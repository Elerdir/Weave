-- Per-conversation LLM runtime override.
-- NULL/default = use global app setting.
ALTER TABLE conversation_settings ADD COLUMN runtime_backend TEXT;
