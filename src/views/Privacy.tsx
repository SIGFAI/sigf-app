// Privacy (docs/PRIVACY.md): the first-start screen and the Privacy sheet. Same text, same choices. Keep the wording
// in step with docs/PRIVACY.md and src-tauri/windows/privacy.txt (the installer's first page).
import { useState, type ReactNode } from 'react';
import { openUrl } from '../lib/api';
import { getPrivacy, savePrivacy, type Privacy } from '../lib/privacy';
import { LOCALES, LOCALE_NAMES, getLocale, setLocale, t, tIn, tx, type Locale } from '../i18n';
import { Icon } from '../ui';

export const POLICY_URL = 'https://sigf.ai/privacy';

function Toggle({ on, onChange, disabled, title, children }: { on: boolean; onChange: (v: boolean) => void; disabled?: boolean; title: string; children: ReactNode }) {
  return (
    <label className={`pv-row ${disabled ? 'pv-off' : ''}`}>
      <input type="checkbox" checked={on && !disabled} disabled={disabled} onChange={(e) => onChange(e.target.checked)} />
      <i className="pv-switch" aria-hidden />
      <span>
        <b>{title}</b>
        <small>{children}</small>
      </span>
    </label>
  );
}

/**
 * The choices and what they mean. `first`: the first-start screen (nothing but the catalog request has gone out yet;
 * "Continue" saves and lets the app start its other requests). Otherwise the Privacy sheet, saved on "Save".
 */
export function PrivacyPanel({ initial, first = false, onDone, onClose, onReport }: { initial: Privacy; first?: boolean; onDone: (p: Privacy) => void; onClose?: () => void; onReport?: () => void }) {
  // The language is not part of this sheet's choices: it is saved on its own, at once.
  const [p, setP] = useState<Omit<Privacy, 'language'>>(() => {
    const { language: _, ...choices } = initial;
    return choices;
  });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const set = (k: Partial<Omit<Privacy, 'language'>>) => setP((x) => ({ ...x, ...k }));

  // The language applies at once and is saved at once (with the choices in force, not this sheet's unsaved ones).
  const pickLanguage = (language: Locale) => {
    setLocale(language);
    void savePrivacy({ ...(getPrivacy() ?? initial), language }).catch(() => {});
  };

  const done = async () => {
    setBusy(true);
    setError(null);
    try {
      onDone(await savePrivacy({ ...p, language: (getPrivacy() ?? initial).language, artSearch: p.storeArt && p.artSearch, asked: true }));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
    setBusy(false);
  };

  return (
    <div className="scrim scrim-center pv-scrim" onClick={first ? undefined : onClose}>
      <div className="join-sheet pv-sheet" onClick={(e) => e.stopPropagation()} role="dialog" aria-labelledby="pv-title">
        {!first && onClose && <button className="detail-close" onClick={onClose} aria-label={t('common.close')}><Icon name="x" size={16} /></button>}
        <div className="pv-body">
          <span className="eyebrow"><Icon name="shield" size={12} /> {t('privacy.eyebrow')}</span>
          <h2 id="pv-title">{first ? t('privacy.titleFirst') : t('privacy.titleSheet')}</h2>
          <p className="muted pv-lead">
            {t('privacy.lead')} {first ? t('privacy.leadFirst') : t('privacy.leadSheet')}
          </p>

          <label className="pv-row pv-row-seg pv-lang">
            <span>
              <b>{t('privacy.language')}{getLocale() === 'en' ? '' : ` · ${tIn('en', 'privacy.language')}`}</b>
            </span>
            <select value={getLocale()} onChange={(e) => pickLanguage(e.target.value as Locale)}>
              {LOCALES.map((l) => <option key={l} value={l} lang={l}>{LOCALE_NAMES[l]}</option>)}
            </select>
          </label>

          <Toggle title={t('privacy.art')} on={p.storeArt} onChange={(v) => set({ storeArt: v })}>
            {t('privacy.artBody')} {t('privacy.artOff')}
          </Toggle>
          <Toggle title={t('privacy.search')} on={p.artSearch} disabled={!p.storeArt} onChange={(v) => set({ artSearch: v })}>
            {t('privacy.searchBody')}
          </Toggle>
          <Toggle title={t('privacy.lobbies')} on={p.lobbyGames} onChange={(v) => set({ lobbyGames: v })}>
            {t('privacy.lobbiesBody')} {t('privacy.lobbiesOff')}
          </Toggle>
          <div className="pv-row pv-row-seg">
            <span>
              <b>{t('privacy.lan')}</b>
              <small>{t('privacy.lanBody')}</small>
            </span>
            <div className="seg">
              <button type="button" className={p.lanAddress === 'ask' ? 'on' : ''} onClick={() => set({ lanAddress: 'ask' })}>{t('privacy.lanAsk')}</button>
              <button type="button" className={p.lanAddress === 'auto' ? 'on' : ''} onClick={() => set({ lanAddress: 'auto' })}>{t('privacy.lanAuto')}</button>
            </div>
          </div>

          <details className="pv-always">
            <summary>{t('privacy.always')}</summary>
            <ul>
              <li>{tx('privacy.alwaysSite', { who: <b>sigf.ai</b> })}</li>
              <li>{tx('privacy.alwaysUpdate', { who: <b>GitHub</b> })}</li>
              <li>{tx('privacy.alwaysDownloads', { who: <b>{t('privacy.whoDownloads')}</b> })}</li>
              <li>{tx('privacy.alwaysOwnCopy', { who: <b>{t('privacy.whoNobody')}</b> })}</li>
              <li>{tx('privacy.alwaysReport', { who: <b>{t('privacy.whoReport')}</b> })}</li>
              <li>{tx('privacy.alwaysServer', { who: <b>{t('privacy.whoServer')}</b> })}</li>
              <li>{tx('privacy.alwaysNever', { who: <b>{t('privacy.whoNever')}</b> })}</li>
            </ul>
          </details>

          {error && <div className="join-error"><span>{error}</span></div>}
          <div className="host-actions pv-actions">
            <a className="pv-link" onClick={() => void openUrl(POLICY_URL)}>{t('privacy.policy')} <Icon name="ext" size={11} /></a>
            {!first && onClose && <button className="act act-ghost" onClick={onClose}>{t('common.cancel')}</button>}
            <button className="act act-get" onClick={() => void done()} disabled={busy} autoFocus>{first ? t('privacy.continue') : t('privacy.save')}</button>
          </div>
          <p className="muted pv-foot">
            {t('privacy.foot')}
            {onReport && <> {t('privacy.footBug')} <a className="rp-link" onClick={onReport}><Icon name="bug" size={11} /> {t('privacy.reportApp')}</a></>}
          </p>
        </div>
      </div>
    </div>
  );
}
