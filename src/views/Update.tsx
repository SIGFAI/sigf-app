// The update banner: "SIGF <version> is available", Update and restart / Later. Nothing downloads before the click,
// and the button waits while a game SIGF modded is running (the core checks the process list) or an install runs.
import { useEffect, useState } from 'react';
import { installUpdate, isUpdateError, onUpdateProgress, updateBlocked, type Available, type UpdateProgress } from '../lib/update';
import { Icon } from '../ui';
import { num, t } from '../i18n';
import { updateErrorText } from '../i18n/errors';

const BLOCK_POLL_MS = 4000;

const mb = (n: number) => t('common.mb', { mb: num(n / 1_048_576, { minimumFractionDigits: 1, maximumFractionDigits: 1 }) });

export function UpdateBanner({ update, gameDirs, beforeInstall, onLater }: {
  update: Available;
  gameDirs: Record<string, string>;
  /** Runs right before the installer closes the app (the hosted lobby is closed there). */
  beforeInstall: () => Promise<void>;
  onLater: () => void;
}) {
  const [blocked, setBlocked] = useState<string | null>(null);
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // While the banner shows: is a game running? Asked again every few seconds, so the button comes back on its own.
  useEffect(() => {
    if (busy) return;
    let live = true;
    const ask = () => updateBlocked(gameDirs).then((b) => live && setBlocked(b ? updateErrorText(b) : null), () => {});
    ask();
    const t = setInterval(ask, BLOCK_POLL_MS);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [busy, gameDirs]);

  useEffect(() => {
    let off: (() => void) | null = null;
    let live = true;
    onUpdateProgress(setProgress).then((f) => (live ? (off = f) : f()));
    return () => {
      live = false;
      off?.();
    };
  }, []);

  const go = async () => {
    setBusy(true);
    setError(null);
    setProgress({ got: 0, total: null, installing: false });
    try {
      await beforeInstall();
      await installUpdate(gameDirs);
    } catch (e) {
      // A game started while it downloaded: the button explains; anything else shows under the title.
      if (isUpdateError(e) && (e.kind === 'gameRunning' || e.kind === 'busy')) setBlocked(updateErrorText(e));
      else setError(isUpdateError(e) ? updateErrorText(e) : String(e));
      setProgress(null);
      setBusy(false);
    }
  };

  const pct = progress?.total ? Math.min(100, (progress.got / progress.total) * 100) : null;
  const status = !progress ? null
    : progress.installing ? t('update.installing')
    : progress.total ? t('update.downloadingOf', { got: mb(progress.got), total: mb(progress.total) })
    : t('update.downloading', { got: mb(progress.got) });

  return (
    <div className="update-banner" role="status">
      <Icon name="download" size={16} />
      <div className="update-text">
        <b>{t('update.available', { version: update.version })}</b>
        <small>{error ?? status ?? t('update.youHave', { version: update.current })}</small>
        {busy && <i className="update-bar" style={{ ['--p' as string]: pct === null ? '35%' : `${pct}%` }} data-indeterminate={pct === null || undefined} />}
      </div>
      {!busy && (
        <>
          <button className="act act-get" onClick={() => void go()} disabled={!!blocked} title={blocked ?? undefined}>
            {blocked ?? t('update.restart')}
          </button>
          <button className="act act-ghost" onClick={onLater}>{t('update.later')}</button>
        </>
      )}
    </div>
  );
}
