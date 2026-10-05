// Bridge to the Rust install engine (app/src-tauri/src/install). In a plain browser every call is a no-op,
// so the UI can render install buttons without the app.

import { inTauri } from './api';

export type Strategy = 'args' | 'mrpack' | 'profile' | 'game-dir-snapshot';
export type Phase = 'download' | 'verify' | 'install' | 'ready';
/** `pct` is overall progress 0..100 across all phases. */
export type InstallProgress = { id: string; phase: Phase; pct: number };

export type InstalledGame = {
  game: string;
  strategy: Strategy;
  launchArgs: string[];
  instance?: string | null;
  profileDir?: string | null;
  snapshot?: string | null;
  gameDir?: string | null;
  launcher?: string | null;
};

export type InstalledMod = { id: string; version: string; name: string; installedAt: number; games: InstalledGame[] };

/** Typed errors from the engine; every variant also carries a readable `message`. */
export type InstallError = { message: string } & (
  | { kind: 'recipe' }
  | { kind: 'download'; url: string }
  | { kind: 'shaMismatch'; file: string; expected: string; actual: string }
  | { kind: 'needsLauncher'; game: string; launcher: 'prism' }
  | { kind: 'missingGameDir'; game: string }
  | { kind: 'pathTraversal'; dst: string }
  | { kind: 'tampered'; files: string[] }
  | { kind: 'snapshotCorrupt'; files: string[] }
  | { kind: 'notInstalled'; id: string }
  | { kind: 'io'; path: string }
);

export function isInstallError(e: unknown): e is InstallError {
  return typeof e === 'object' && e !== null && 'kind' in e && 'message' in e;
}

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(cmd, args);
}

/**
 * Installs a recipe (`mashup.json` text). `gameDirs` maps canonical game id -> scanned install dir
 * (required for `game-dir-snapshot` games). Rejects with an `InstallError`. Browser: resolves to null.
 */
export async function installMashup(recipeJson: string, gameDirs: Record<string, string>): Promise<InstalledMod | null> {
  if (!inTauri) return null;
  return invoke<InstalledMod>('install', { recipeJson, gameDirs });
}

/** "Restore vanilla". Without `force` it rejects with `tampered` if game files changed since install. */
export async function restoreMashup(id: string, force = false): Promise<void> {
  if (!inTauri) return;
  return invoke<void>('restore', { id, force });
}

export async function installedMods(): Promise<InstalledMod[]> {
  if (!inTauri) return [];
  return invoke<InstalledMod[]>('installed');
}

/** Subscribes to `install://progress`; returns the unsubscribe function. Browser: never fires. */
export async function onInstallProgress(cb: (p: InstallProgress) => void): Promise<() => void> {
  if (!inTauri) return () => {};
  const { listen } = await import('@tauri-apps/api/event');
  return listen<InstallProgress>('install://progress', (e) => cb(e.payload));
}
