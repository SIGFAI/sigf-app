import type { Ctx } from '../App';
import { isCommunity, type Mashup } from '../data/catalog';
import { GAME } from '../data/games';
import { openUrl } from '../lib/api';
import { Avatar, GameArt, Icon, MashupCover, fmtCount, shownDownloads } from '../ui';
import { ActionButton, gameName } from './shared';
import { PlayTogether } from './Lobbies';
import { date, list, t, tx } from '../i18n';

/** Taglines may carry `**bold**` from the catalog: render it, never show the asterisks. */
function bold(text: string) {
  return text.split(/\*\*(.+?)\*\*/g).map((part, i) => (i % 2 ? <b key={i}>{part}</b> : part));
}

export function Detail({ ctx, m, onClose }: { ctx: Ctx; m: Mashup; onClose: () => void }) {
  const installed = ctx.installs[m.id]?.phase === 'ready';
  const sides = [m.host, m.guest].filter(Boolean) as string[];
  const author = m.links?.author ?? null;
  const repo = m.links?.repo ?? m.repo ?? null;
  const howTo = m.howToPlay ?? [];
  const notes = m.notes ?? [];
  const own = m.ownCopies ?? [];
  const builds = m.playerBuild ?? [];
  // Mashups that change the same game files: never installed together (the core refuses the pair either way).
  const conflicts = (m.conflicts ?? []).map((id) => ctx.catalog.find((c) => c.id === id)?.name ?? id.replace(/^sigf\//, ''));
  const clash = installed ? null : ctx.conflictOf(m);
  // A community mashup (credited author, not SIGF or a launchpad agent): reviewed by SIGF before it is listed.
  const community = !m.by.agent && m.by.name !== 'SIGF';
  // Submitted through sigf.ai/submit: "Community · by <GitHub owner>".
  const submitted = isCommunity(m);

  return (
    <div className="scrim" onClick={onClose}>
      <aside className="detail" onClick={(e) => e.stopPropagation()}>
        <button className="detail-close" onClick={onClose} aria-label={t('common.close')}><Icon name="x" size={16} /></button>
        <MashupCover host={m.host} guest={m.guest} cover={m.cover} className="detail-cover" />
        <div className="detail-body">
          <div className="card-pair">
            {m.subtitle ?? <>{gameName(m.host)} <b>×</b> {GAME[m.guest ?? '']?.short ?? m.guest}</>}
            {m.kind === 'passthrough' && <span className="chip chip-prism">{t('card.crossover')}</span>}
          </div>
          <h1>{m.name}</h1>
          <p className="lede">{bold(m.tagline)}</p>
          <div className="byline">
            <span className="by-line">
              <Avatar src={m.avatar} size={18} />
              {tx(submitted ? 'detail.byCommunity' : 'detail.by', { author: author ? <a onClick={() => openUrl(author)}>{m.by.name}</a> : <b>{m.by.name}</b> })}
            </span>
            {repo && <a onClick={() => openUrl(repo)}>{t('detail.source')} <Icon name="ext" size={11} /></a>}
            <a className="rp-link" onClick={() => ctx.report(m)}><Icon name="bug" size={12} /> {t('report.action')}</a>
            {m.status === 'beta' && <span className="chip">{t('card.beta')}</span>}
          </div>

          <div className="detail-cta">
            <ActionButton ctx={ctx} m={m} big />
            {installed && (
              <button className="act act-ghost" onClick={() => ctx.restore(m)}>
                <Icon name="restore" size={15} /> {t('detail.restore')}
              </button>
            )}
            {installed && (
              <button className="act act-ghost" onClick={() => ctx.report(m)} title={t('detail.reportTitle')}>
                <Icon name="bug" size={15} /> {t('report.action')}
              </button>
            )}
            <button
              className="act act-ghost"
              title={t('detail.remixTitle')}
              onClick={() => {
                ctx.setPair([m.host, m.guest && GAME[m.guest] ? m.guest : null]);
                onClose();
                ctx.go('build');
              }}
            >
              <Icon name="build" size={15} /> {t('detail.remix')}
            </button>
          </div>

          {clash && (
            <div className="trust trust-warn">
              <Icon name="restore" size={16} />
              <span>
                {t('detail.clash', { other: clash.name, name: m.name })}{' '}
                <button className="act act-ghost" onClick={() => ctx.restoreThenGet(m, clash)}>
                  {t('detail.restoreFirst', { other: clash.name })}
                </button>
              </span>
            </div>
          )}

          {howTo.length > 0 && (
            <>
              <h4>{t('detail.howTo')}</h4>
              <ol className="notes howto">
                {howTo.map((n, i) => <li key={i}>{n}</li>)}
              </ol>
            </>
          )}

          {notes.length > 0 && (
            <>
              <h4>{t('detail.before')}</h4>
              <ul className="notes">
                {notes.map((n, i) => <li key={i} className={i === 0 && /^how it works:/i.test(n) ? 'notes-lead' : undefined}>{n}</li>)}
              </ul>
            </>
          )}

          {conflicts.length > 0 && (
            <>
              <h4>{t('detail.notTogether')}</h4>
              <ul className="notes">
                <li>{t('detail.conflicts', { list: list(conflicts), count: conflicts.length })}</li>
              </ul>
            </>
          )}

          <PlayTogether ctx={ctx} m={m} />

          <h4>{t('detail.needs')}</h4>
          <div className="needs">
            {sides.map((id) => {
              const known = !!GAME[id];
              const required = m.needs.includes(id);
              const copy = own.find((c) => c.game === id);
              const have = ctx.owned.has(id);
              const g = ctx.scan?.games.find((x) => x.canon === id);
              return (
                <div key={id} className={`need ${required ? (have ? 'have' : 'lack') : 'content'}`}>
                  <GameArt id={known ? id : null} name={GAME[id]?.name ?? id} src={g?.art} wide={g?.artWide} local={g?.artLocal} wideLocal={g?.wideLocal} />
                  <div>
                    <b>{GAME[id]?.name ?? id}</b>
                    {copy ? (
                      <span>{t('detail.ownCopy', { label: copy.label })}</span>
                    ) : required ? (
                      have ? (
                        <span><Icon name="check" size={13} /> {t('detail.onPc')}{g?.build ? ` · ${t('lib.build', { build: g.build })}` : ''}</span>
                      ) : (
                        <span>
                          {t('detail.notFound')} ·{' '}
                          <a onClick={() => GAME[id]?.store && openUrl(GAME[id].store!)}>{t('detail.whereToGet')} <Icon name="ext" size={11} /></a>
                        </span>
                      )
                    ) : (
                      <span>{t('detail.notNeeded')}</span>
                    )}
                  </div>
                </div>
              );
            })}
          </div>

          {(own.length > 0 || builds.length > 0) && (
            <div className="trust">
              <Icon name="shield" size={16} />
              <span>
                {own.map((c) => `${t('detail.trustOwn', { label: c.label })} `).join('')}
                {builds.map((b) => `${b.minutes ? t('detail.trustBuildMin', { label: b.label, count: b.minutes }) : t('detail.trustBuild', { label: b.label })} `).join('')}
                {t('detail.trustRestore')}
              </span>
            </div>
          )}

          {m.steps.length > 0 && (
            <>
              <h4>{t('detail.steps')}</h4>
              <ol className="steps">
                {m.steps.map((s) => <li key={s}>{s}</li>)}
              </ol>
            </>
          )}

          <div className="facts">
            {m.strategy && <div><small>{t('detail.method')}</small>{m.strategy}</div>}
            {m.sizeMb > 0 && <div><small>{t('detail.size')}</small>{t('common.mb', { mb: m.sizeMb })}</div>}
            <div><small>{t('detail.madeBy')}</small><span className="by-line"><Avatar src={m.avatar} size={20} />{m.links?.author ? <a onClick={() => openUrl(m.links!.author!)}>{m.by.name} <Icon name="ext" size={11} /></a> : m.by.name}{m.by.model ? ` · ${m.by.model}` : ''}</span></div>
            {shownDownloads(m.downloads) && <div title={t('detail.downloadsTitle')}><small>{t('detail.downloads')}</small>{fmtCount(m.downloads)}</div>}
            {m.license && <div><small>{t('detail.license')}</small>{m.license}</div>}
            {m.updated && <div><small>{t('detail.updated')}</small>{date(m.updated)}</div>}
            {m.repo && <div><small>{t('detail.source')}</small><a onClick={() => openUrl(m.repo!)}>GitHub <Icon name="ext" size={11} /></a></div>}
            {m.links?.releases && <div><small>{t('detail.releases')}</small><a onClick={() => openUrl(m.links!.releases!)}>GitHub <Icon name="ext" size={11} /></a></div>}
            <div><small>{t('detail.foundBug')}</small><a onClick={() => ctx.report(m)}>{t('detail.reportIt')}</a></div>
          </div>

          {m.status === 'beta' && (
            <div className="trust">
              <Icon name="shield" size={16} />
              <span>{t(submitted ? 'detail.betaSubmitted' : 'detail.beta', { author: m.by.name })}</span>
            </div>
          )}

          <div className="trust">
            <Icon name="shield" size={16} />
            <span>{community && `${t('detail.trustReviewed')} `}{t('detail.trust')}</span>
          </div>
        </div>
      </aside>
    </div>
  );
}
