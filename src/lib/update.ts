// In-app updates (src-tauri/src/update.rs): the core asks GitHub for the latest version, checks the download against
// the release key and runs the installer. The UI only asks, shows the banner and waits for the player's click.
import { inTauri } from './api';

export type Available = { version: string; current: string; notes?: string | null };
export type UpdateProgress = { got: number; total?: number | null; installing: boolean };
export type UpdateError = { kind: 'gameRunning' | 'busy' | 'privacy' | 'none' | 'failed'; message: string };

/** How often the app asks again while it stays open. */
export const UPDATE_EVERY_MS = 6 * 60 * 60_000;

export const isUpdateError = (e: unknown): e is UpdateError => typeof e === 'object' && e !== null && 'kind' in e && 'message' in e;

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(cmd, args);
}

/** The newer version, or null when this one is the latest (or in the browser preview). */
export async function checkUpdate(): Promise<Available | null> {
  if (!inTauri) return null;
  return invoke<Available | null>('update_check');
}

/** Why the update has to wait right now (a game running, an install), or null. `gameDirs`: game id -> install folder. */
export async function updateBlocked(gameDirs: Record<string, string>): Promise<UpdateError | null> {
  if (!inTauri) return null;
  return invoke<UpdateError | null>('update_blocked', { gameDirs });
}

/** Downloads and installs the update the last check found; the app closes and the new version starts. */
export async function installUpdate(gameDirs: Record<string, string>): Promise<void> {
  if (!inTauri) return;
  await invoke<void>('update_install', { gameDirs });
}

export async function onUpdateProgress(cb: (p: UpdateProgress) => void): Promise<() => void> {
  if (!inTauri) return () => {};
  const { listen } = await import('@tauri-apps/api/event');
  return listen<UpdateProgress>('update://progress', (e) => cb(e.payload));
}
