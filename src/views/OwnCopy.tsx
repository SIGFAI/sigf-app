// Bring your own copy (the recipe's `own_copies`): the app looked for the player's own file (a ROM they dumped) on this PC
// and did not find a dump it accepts. The player picks it in the native dialog the core opens; the core checks it
// (SHA-1) and copies it into the mashup's own folder. The file never leaves the PC.

import { useState } from 'react';
import type { Mashup } from '../data/catalog';
import { isInstallError, pickOwnCopy, type OwnFound } from '../lib/install';
import { Icon, MashupCover } from '../ui';
import { list, t } from '../i18n';
import { installErrorText } from '../i18n/errors';

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
          ? t('own.mismatch', { label: e.label, sha1: e.sha1 })
          : installErrorText(e, { asSent: true }),
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="scrim scrim-center" onClick={() => onDone(false)}>
      <div className="join-sheet" onClick={(e) => e.stopPropagation()}>
        <MashupCover host={ask.m.host} guest={ask.m.guest} cover={ask.m.cover} className="join-cover" />
        <button className="detail-close" onClick={() => onDone(false)} aria-label={t('common.close')}><Icon name="x" size={16} /></button>
        <div className="join-body own-copy">
          <span className="eyebrow">{t('own.eyebrow')}</span>
          <h2>{t('own.title', { name: ask.m.name, label: cur.label })}</h2>
          <p className="own-lede">{t('own.lede', { label: cur.label })}</p>
          <ul className="notes">
            <li>
              {cur.rejected.length
                ? t('own.found', { files: list(cur.rejected.length > 3 ? [...cur.rejected.slice(0, 3), t('own.moreFiles', { count: cur.rejected.length - 3 })] : cur.rejected) })
                : t('own.notFound')}
            </li>
            <li>{t('own.pick')}</li>
          </ul>
          {error && <div className="join-error"><span>{error}</span></div>}
          <div className="host-actions">
            <button className="act act-ghost" onClick={() => onDone(false)}>{t('common.cancel')}</button>
            <button className="act act-get" disabled={busy} onClick={() => void pick()}>{busy ? t('own.checking') : t('own.choose')}</button>
          </div>
        </div>
      </div>
    </div>
  );
}
