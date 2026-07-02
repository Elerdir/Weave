import { invoke } from "@tauri-apps/api/core";

export interface LogEntry {
  timestamp: string;
  level: string;
  target: string;
  message: string;
}

export type LogLevel = "error" | "warn" | "info" | "debug" | "trace";

const DEFAULT_LIMIT = 500;

function createLogsStore() {
  let entries = $state<LogEntry[]>([]);
  let loading = $state(false);
  let minLevel = $state<LogLevel | "">("");
  let target = $state("");
  let search = $state("");

  return {
    get entries() {
      return entries;
    },
    get loading() {
      return loading;
    },
    get minLevel() {
      return minLevel;
    },
    set minLevel(v: LogLevel | "") {
      minLevel = v;
    },
    get target() {
      return target;
    },
    set target(v: string) {
      target = v;
    },
    get search() {
      return search;
    },
    set search(v: string) {
      search = v;
    },
    /** Kořenové moduly (crate) z načtených záznamů — pro filtr modulů. */
    get targets(): string[] {
      const roots = new Set(entries.map((e) => e.target.split("::")[0]));
      return [...roots].sort();
    },

    async load() {
      loading = true;
      try {
        entries = await invoke<LogEntry[]>("get_app_logs", {
          minLevel: minLevel || null,
          target: target || null,
          search: search || null,
          limit: DEFAULT_LIMIT,
        });
      } finally {
        loading = false;
      }
    },
  };
}

export const logsStore = createLogsStore();
