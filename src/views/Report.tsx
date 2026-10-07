// "Report a bug" (src-tauri/src/report.rs, docs/PRIVACY.md): the report is written on this PC and shown here first.
// SIGF sends nothing: "Open on GitHub" opens the new-issue page in the player's browser, where they submit it or not.
// The player picks the kind of problem: a bug in the mod goes to its author's tracker, an install or app problem to the
// SIGFAI copy the app installs from.
import { useEffect, useState } from 'react';
import type { Ctx } from '../App';
import type { Mashup } from '../data/catalog';
import { GAME } from '../data/games';
import { bugReport, copyText, openUrl, type BugReport, type LastError, type ReportInput, type ReportKind } from '../lib/api';
import { Icon, STORE_LABEL } from '../ui';
import { t, type Key } from '../i18n';

/** What the UI knows for the report: the card, the last scan, the install in progress and the last error shown. */
export function reportInput(ctx: Ctx, m: Mashup | null, last: LastError | null): ReportInput {
  const lastError = last?.message ?? null;
  const lastErrorKind = last?.kind ?? null;
  if (!m) {
    const games = (ctx.scan?.stores ?? []).map((s) => {
      const n = ctx.scan!.games.filter((g) => g.store === s).length;
      return { id: s, name: STORE_LABEL[s] ?? s, store: `${n} game${n === 1 ? '' : 's'}`, build: null };
    });
    for (const l of ctx.scan?.launchers ?? []) games.push({ id: l.kind, name: l.kind === 'prism' ? 'Prism Launcher' : l.kind === 'modrinth' ? 'Modrinth App' : 'Minecraft Launcher', store: 'found', build: null });
    return { mashup: null, games, progress: null, lastError, lastErrorKind };
  }
  const sides = [m.host, m.guest].filter(Boolean) as string[];
  const games = sides.map((id) => {
    const g = ctx.scan?.games.find((x) => x.canon === id);
    return { id, name: GAME[id]?.name ?? id, store: g ? STORE_LABEL[g.store] ?? g.store : null, build: g?.build ?? null, optional: !m.needs.includes(id) };
  });
  const i = ctx.installs[m.id];
  const progress = i && i.phase !== 'ready' ? `${i.phase} ${Math.round(i.pct)}%` : null;
  return {
    mashup: { id: m.id, name: m.name, version: m.version ?? null, links: m.links ? { issues: m.links.issues } : null, repo: m.links?.repo ?? m.repo ?? null },
    games,
    progress,
    lastError,
    lastErrorKind,
  };
}

const KIND_LABEL: Record<ReportKind, Key> = { mod: 'report.kindMod', install: 'report.kindInstall', app: 'report.kindApp' };

export function ReportSheet({ ctx, m, lastError, onClose, flash }: { ctx: Ctx; m: Mashup | null; lastError: LastError | null; onClose: () => void; flash: (s: string) => void }) {
  const [r, setR] = useState<BugReport | null>(null);
  const [kind, setKind] = useState<ReportKind | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    // Written once when the sheet opens: what the player reviews is what goes.
    bugReport(reportInput(ctx, m, lastError)).then(
      (x) => {
        if (!live) return;
        setR(x);
        setKind(x.preselect);
      },
      (e) => live && setError(String(e)),
    );
    return () => { live = false; };
  }, []);

  const tg = r?.targets.find((x) => x.kind === kind) ?? null;
  const copy = async () => {
    if (r && (await copyText(r.full))) flash(t('toast.reportCopied'));
    else flash(t('toast.copyFailed'));
  };
  const open = () => {
    if (!tg) return;
    void openUrl(tg.url);
    onClose();
  };
  const whose = (k: ReportKind, tracker: string) =>
    k === 'mod' && m ? t('report.authorTracker', { author: m.by.name, tracker }) : t('report.sigfCopy', { tracker });

  return (
    <div className="scrim scrim-center pv-scrim" onClick={onClose}>
      <div className="join-sheet pv-sheet rp-sheet" onClick={(e) => e.stopPropagation()} role="dialog" aria-labelledby="rp-title">
        <button className="detail-close" onClick={onClose} aria-label={t('common.close')}><Icon name="x" size={16} /></button>
        <div className="pv-body">
          <span className="eyebrow"><Icon name="bug" size={12} /> {t('report.action')}</span>
          <h2 id="rp-title">{m ? m.name : t('report.theApp')}</h2>
          {r && r.targets.length > 1 && (
            <div className="seg rp-kinds" role="radiogroup" aria-label={t('report.kindLabel')}>
              {r.targets.map((x) => (
                <button key={x.kind} type="button" role="radio" aria-checked={kind === x.kind} className={kind === x.kind ? 'on' : ''} onClick={() => setKind(x.kind)} title={whose(x.kind, x.tracker)}>
                  {t(KIND_LABEL[x.kind])}
                </button>
              ))}
            </div>
          )}
          {tg && (
            <p className="muted pv-lead">
              {t('report.lead', { where: tg.kind === 'app' ? t('report.appTracker', { tracker: tg.tracker }) : whose(tg.kind, tg.tracker) })}
              {m && r && r.targets.length === 1 && tg.kind === 'install' && m.links && !m.links.issues ? ` ${t('report.noAuthorTracker', { author: m.by.name })}` : ''}
            </p>
          )}
          {r && !tg && (
            <p className="muted pv-lead">
              {t('report.noTracker', { name: m?.name ?? t('report.thisMashup') })}
            </p>
          )}
          {!r && !error && <p className="muted pv-lead">{t('report.writing')}</p>}
          {error && <div className="join-error"><span>{error}</span></div>}
          {r && (
            <>
              <div className="rp-title"><small>{t('report.title')}</small><span>{r.title}<i>{t('report.addWords')}</i></span></div>
              <pre className="rp-text" tabIndex={0}>{tg ? tg.body : r.full}</pre>
              {tg?.truncated && <p className="muted rp-note">{t('report.truncated')}</p>}
              <p className="muted rp-note">{t('report.scrubbed')}</p>
            </>
          )}
          <div className="host-actions pv-actions">
            <button className="act act-ghost" onClick={onClose}>{t('common.cancel')}</button>
            <button className={`act ${tg ? 'act-ghost' : 'act-get'}`} onClick={() => void copy()} disabled={!r}>
              <Icon name="copy" size={15} /> {t('report.copy')}
            </button>
            {tg && (
              <button className="act act-get" onClick={open} autoFocus>
                {t('report.open')} <Icon name="ext" size={13} />
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
