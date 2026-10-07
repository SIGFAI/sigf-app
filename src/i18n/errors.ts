// The core's errors in the player's language. The core sends a stable `kind` (and fields) with an English `message`:
// English shows that message as before; other languages translate the kinds they know and keep the English details.
import { isInstallError } from '../lib/install';
import { getLocale, list, tIn, type Key, type Locale } from '.';
import { en } from './en';

/** `prefix + kind` when the dictionaries have it. */
const known = (prefix: string, kind: string): Key | null => {
  const k = `${prefix}${kind}`;
  return k in en ? (k as Key) : null;
};

/**
 * An install, Get or restore error, said for the player. `loc` 'en' gives the text kept for bug reports. `asSent`: in
 * English every kind shows the core's own message (the other languages are unchanged).
 */
export function installErrorText(e: unknown, { loc = getLocale(), asSent = false }: { loc?: Locale; asSent?: boolean } = {}): string {
  if (!isInstallError(e)) return String(e);
  if (loc === 'en' && asSent) return e.message;
  switch (e.kind) {
    case 'needsLauncher':
      return tIn(loc, 'err.install.needsLauncher');
    case 'ownCopyMissing':
      return tIn(loc, 'err.install.ownCopyMissing', { label: e.label });
    case 'ownCopyMismatch':
      return tIn(loc, 'err.install.ownCopyMismatch', { label: e.label });
    case 'buildFailed':
      return e.log ? tIn(loc, 'err.install.buildFailedLog', { label: e.label, message: e.message, log: e.log }) : tIn(loc, 'err.install.buildFailed', { label: e.label, message: e.message });
    case 'conflict':
      return tIn(loc, 'err.install.conflict', { name: e.name, withName: e.withName });
  }
  const key = known('err.install.', e.kind);
  if (loc === 'en' || !key) return e.message;
  const vars: Record<string, string> = { message: e.message };
  for (const [k, v] of Object.entries(e)) {
    if (typeof v === 'string') vars[k] = v;
    else if (Array.isArray(v)) vars[k] = list(v, 'conjunction', loc);
  }
  return tIn(loc, key, vars);
}

/** Play failed to start a game (`launch`): the core's text, introduced in the player's language. */
export function playErrorText(message: string, loc: Locale = getLocale()): string {
  return loc === 'en' ? message : tIn(loc, 'err.play.launch', { message });
}

// Join kinds the lobby service also reports: one text for both.
const JOIN_AS_API = new Map<string, Key>([['lobbyNotFound', 'lobbyApi.not_found'], ['lobbyClosed', 'lobbyApi.lobby_closed']]);

/** A join error (`JoinError`: lobby kinds, or an install or launch error's kind). */
export function joinErrorText(e: { kind: string; message: string }, loc: Locale = getLocale()): string {
  if (loc === 'en') return e.message;
  return tIn(loc, JOIN_AS_API.get(e.kind) ?? known('err.join.', e.kind) ?? 'err.join.other', { message: e.message });
}

/** Why the update waits or failed (`UpdateError`). */
export function updateErrorText(e: { kind: string; message: string }, loc: Locale = getLocale()): string {
  if (loc === 'en') return e.message;
  return tIn(loc, known('err.update.', e.kind) ?? 'err.update.failed', { message: e.message });
}
