import "@testing-library/jest-dom/vitest";
import { vi } from "vitest";

// Mock Tauri API — v testech nevoláme skutečné Rust commands
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
  emit: vi.fn(),
}));
