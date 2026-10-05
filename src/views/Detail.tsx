import type { Ctx } from '../App';
import type { Mashup } from '../data/catalog';
import { GAME } from '../data/games';
import { openUrl } from '../lib/api';
import { Avatar, GameArt, Icon, MashupCover, fmtCount } from '../ui';
import { ActionButton, gameName } from './shared';
import { PlayTogether } from './Lobbies';

export function Detail({ ctx, m, onClose }: { ctx: Ctx; m: Mashup; onClose: () => void }) {
  const installed = ctx.installs[m.id]?.phase === 'ready';
  const sides = [m.host, m.guest].filter(Boolean) as string[];

  return (
    <div className="scrim" onClick={onClose}>
      <aside className="detail" onClick={(e) => e.stopPropagation()}>
        <button className="detail-close" onClick={onClose} aria-label="Close"><Icon name="x" size={16} /></button>
        <MashupCover host={m.host} guest={m.guest} cover={m.cover} className="detail-cover" />
        <div className="detail-body">
          <div className="card-pair">
            {m.subtitle ?? <>{gameName(m.host)} <b>×</b> {GAME[m.guest ?? '']?.short ?? m.guest}</>}
            {m.kind === 'passthrough' && <span className="chip chip-prism">Real crossover</span>}
          </div>
          <h1>{m.name}</h1>
          <p className="lede">{m.tagline}</p>

          <div className="detail-cta">
            <ActionButton ctx={ctx} m={m} big />
            {installed && (
              <button className="act act-ghost" onClick={() => ctx.restore(m)}>
                <Icon name="restore" size={15} /> Restore vanilla
              </button>
            )}
            <button
              className="act act-ghost"
              title="Same pair, your twist: an agent builds your version"
              onClick={() => {
                ctx.setPair([m.host, m.guest && GAME[m.guest] ? m.guest : null]);
                onClose();
                ctx.go('build');
              }}
            >
              <Icon name="build" size={15} /> Remix
            </button>
          </div>

          <PlayTogether ctx={ctx} m={m} />

          <h4>What you need</h4>
          <div className="needs">
            {sides.map((id) => {
              const known = !!GAME[id];
              const required = m.needs.includes(id);
              const have = ctx.owned.has(id);
              const g = ctx.scan?.games.find((x) => x.canon === id);
              return (
                <div key={id} className={`need ${required ? (have ? 'have' : 'lack') : 'content'}`}>
                  <GameArt id={known ? id : null} name={GAME[id]?.name ?? id} src={g?.art} wide={g?.artWide} local={g?.artLocal} wideLocal={g?.wideLocal} />
                  <div>
                    <b>{GAME[id]?.name ?? id}</b>
                    {required ? (
                      have ? (
                        <span><Icon name="check" size={13} /> On this PC{g?.build ? ` · build ${g.build}` : ''}</span>
                      ) : (
                        <span>
                          Not found ·{' '}
                          <a onClick={() => GAME[id]?.store && openUrl(GAME[id].store!)}>where to get it <Icon name="ext" size={11} /></a>
                        </span>
                      )
                    ) : (
                      <span>Not needed: its look and mechanics come inside the mod</span>
                    )}
                  </div>
                </div>
              );
            })}
          </div>

          {m.steps.length > 0 && (
            <>
              <h4>What the install does</h4>
              <ol className="steps">
                {m.steps.map((s) => <li key={s}>{s}</li>)}
              </ol>
            </>
          )}

          <div className="facts">
            {m.strategy && <div><small>Method</small>{m.strategy}</div>}
            {m.sizeMb > 0 && <div><small>Size</small>{m.sizeMb} MB</div>}
            <div><small>Made by</small><span className="by-line"><Avatar src={m.avatar} size={20} />{m.links?.author ? <a onClick={() => openUrl(m.links!.author!)}>{m.by.name} <Icon name="ext" size={11} /></a> : m.by.name}{m.by.model ? ` · ${m.by.model}` : ''}</span></div>
            {m.plays > 0 && <div><small>Plays</small>{fmtCount(m.plays)}{m.rating > 0 && ` · ★ ${m.rating.toFixed(1)}`}</div>}
            {m.license && <div><small>License</small>{m.license}</div>}
            {m.updated && <div><small>Updated</small>{m.updated.slice(0, 10)}</div>}
            {m.repo && <div><small>Source</small><a onClick={() => openUrl(m.repo!)}>GitHub <Icon name="ext" size={11} /></a></div>}
            {m.links?.releases && <div><small>Releases</small><a onClick={() => openUrl(m.links!.releases!)}>GitHub <Icon name="ext" size={11} /></a></div>}
            {m.links?.issues && <div><small>Found a bug?</small><a onClick={() => openUrl(m.links!.issues!)}>Report it to {m.by.name} <Icon name="ext" size={11} /></a></div>}
          </div>

          {m.status === 'beta' && (
            <div className="trust">
              <Icon name="shield" size={16} />
              <span>Beta: a community mashup by {m.by.name}, packaged by SIGF with credit. Expect rough edges{m.links?.issues ? ', and report bugs to the author with the link above' : ''}.</span>
            </div>
          )}

          <div className="trust">
            <Icon name="shield" size={16} />
            <span>Scanned for malware and smoke-tested on a clean machine before listing. Every file is hash-checked. Your game files are snapshotted before anything changes, and Restore vanilla puts them back. Never launched into a game's official online mode.</span>
          </div>
        </div>
      </aside>
    </div>
  );
}
