// Bring your own copy (the recipe's `own_copies`): the app looked for the player's own file (a ROM they dumped) on this PC
// and did not find a dump it accepts. The player picks it in the native dialog the core opens; the core checks it
// (SHA-1) and copies it into the mashup's own folder. The file never leaves the PC.

import { useState } from 'react';
import type { Mashup } from '../data/catalog';
import { isInstallError, pickOwnCopy, type OwnFound } from '../lib/install';
import { Icon, MashupCover } from '../ui';

export type OwnAsk = { m: Mashup; recipe: string; missing: OwnFound[]; resolve: (ok: boolean) => void };

export function OwnCopySheet({ ask, onDone }: { ask: OwnAsk; onDone: (ok: boolean) => void }) {
  const [left, setLeft] = useState(ask.missing);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const cur = left[0];
  if (!cur) return null;

  const pick = async () => {
    setBusy(true);
    setError(null);
    try {
      const got = await pickOwnCopy(ask.recipe, cur.game);
      if (!got?.found) return; // dialog closed
      const rest = left.slice(1);
      if (!rest.length) return onDone(true);
      setLeft(rest);
    } catch (e) {
      setError(
        isInstallError(e) && e.kind === 'ownCopyMismatch'
          ? `This file is not the ${e.label} dump this mashup needs (SHA-1 ${e.sha1}). Pick a clean, unmodified dump: other regions, revisions and patched ROMs don't work.`
          : isInstallError(e) ? e.message : String(e),
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="scrim scrim-center" onClick={() => onDone(false)}>
      <div className="join-sheet" onClick={(e) => e.stopPropagation()}>
        <MashupCover host={ask.m.host} guest={ask.m.guest} cover={ask.m.cover} className="join-cover" />
        <button className="detail-close" onClick={() => onDone(false)} aria-label="Close"><Icon name="x" size={16} /></button>
        <div className="join-body own-copy">
          <span className="eyebrow">Your own copy</span>
          <h2>{ask.m.name} needs {cur.label}</h2>
          <p className="own-lede">Uses your own copy of {cur.label}. SIGF never ships or downloads it.</p>
          <ul className="notes">
            <li>
              SIGF looked in your Downloads, Desktop, Documents and ROM folders
              {cur.rejected.length ? <> and found {cur.rejected.slice(0, 3).join(', ')}{cur.rejected.length > 3 ? ` and ${cur.rejected.length - 3} more` : ''}, but not the dump this mashup needs.</> : <> and did not find it.</>}
            </li>
            <li>Pick the file (a .zip holding it works too). It stays on your PC: SIGF checks it and copies it into this mashup's folder, and Restore vanilla deletes that copy. Nothing is uploaded.</li>
          </ul>
          {error && <div className="join-error"><span>{error}</span></div>}
          <div className="host-actions">
            <button className="act act-ghost" onClick={() => onDone(false)}>Cancel</button>
            <button className="act act-get" disabled={busy} onClick={() => void pick()}>{busy ? 'Checking…' : 'Choose file…'}</button>
          </div>
        </div>
      </div>
    </div>
  );
}
