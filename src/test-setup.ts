import "@testing-library/jest-dom/vitest";
import { vi } from "vitest";

// Mock Tauri API — v testech nevoláme skutečné Rust commands
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: vi.fn((path: string) => `asset://localhost/${path}`),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
  emit: vi.fn(),
  emitTo: vi.fn(),
}));

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: vi.fn(() => ({
    onDragDropEvent: vi.fn(() => Promise.resolve(() => {})),
  })),
}));

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn(() => Promise.resolve("0.0.0-test")),
}));

vi.mock("@tauri-apps/plugin-updater", () => ({
  check: vi.fn(() => Promise.resolve(null)),
}));

vi.mock("@tauri-apps/plugin-process", () => ({
  relaunch: vi.fn(() => Promise.resolve()),
}));
