import type { Ctx } from '../App';
import { MashupCover } from '../ui';
import { ActionButton, gameName } from './shared';
import { HostedWorlds } from './Lobbies';
import { Icon } from '../ui';
import { t } from '../i18n';

export function Queue({ ctx }: { ctx: Ctx }) {
  const rows = Object.entries(ctx.installs)
    .map(([id, i]) => ({ m: ctx.catalog.find((c) => c.id === id), i }))
    .filter((r) => r.m)
    .sort((x, y) => y.i.started - x.i.started);

  return (
    <div className="page">
      <section className="section">
        <header>
          <div>
            <h2>{t('queue.title')}</h2>
            <p>{t('queue.sub')}</p>
          </div>
          <label className="act act-ghost" title={t('queue.fromFileTitle')}>
            <Icon name="plus" size={15} /> {t('queue.fromFile')}
            <input type="file" accept=".json,application/json" hidden onChange={async (e) => { const f = e.target.files?.[0]; if (f) ctx.sideload(await f.text()); e.target.value = ''; }} />
          </label>
        </header>
        {rows.length === 0 && <div className="empty">{t('queue.empty')}</div>}
        <div className="queue">
          {rows.map(({ m, i }) => (
            <div key={m!.id} className="qrow" onClick={() => ctx.open(m!)}>
              <MashupCover host={m!.host} guest={m!.guest} cover={m!.cover} className="qcover" />
              <div className="qinfo">
                <b>{m!.name}</b>
                <span>{gameName(m!.host)} × {gameName(m!.guest)} · {m!.strategy}</span>
              </div>
              <ActionButton ctx={ctx} m={m!} />
              {i.phase === 'ready' && (
                <button className="act act-ghost" onClick={(e) => { e.stopPropagation(); ctx.report(m!); }} title={t('report.action')} aria-label={t('report.action')}>
                  <Icon name="bug" size={15} />
                </button>
              )}
              {i.phase === 'ready' && (
                <button className="act act-ghost" onClick={(e) => { e.stopPropagation(); ctx.restore(m!); }} title={t('detail.restore')} aria-label={t('detail.restore')}>
                  <Icon name="restore" size={15} />
                </button>
              )}
            </div>
          ))}
        </div>
      </section>
      <HostedWorlds ctx={ctx} />
    </div>
  );
}
