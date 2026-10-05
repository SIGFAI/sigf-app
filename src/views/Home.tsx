import { useMemo, useState } from 'react';
import type { Ctx } from '../App';
import { GAME } from '../data/games';
import { GameArt, Icon } from '../ui';
import { imageOk, usePrivacy } from '../lib/privacy';
import { Card, Section, gameName, missing } from './shared';

type Filter = 'playable' | 'all' | 'crossover';

function Slot({ ctx, slot }: { ctx: Ctx; slot: 0 | 1 }) {
  const id = ctx.pair[slot];
  const g = id ? GAME[id] : null;
  const scanned = id ? ctx.scan?.games.find((x) => x.canon === id) : null;
  return (
    <button className={`slot ${id ? 'slot-full' : ''}`} onClick={() => ctx.pick(slot)}>
      {id ? (
        <>
          <GameArt id={id} name={g?.name ?? id} src={scanned?.art} wide={scanned?.artWide} local={scanned?.artLocal} wideLocal={scanned?.wideLocal} />
          <span className="slot-label">
            <small>{slot === 0 ? 'Host' : 'Guest'}</small>
            {g?.short ?? id}
          </span>
        </>
      ) : (
        <span className="slot-empty">
          <Icon name="plus" size={26} />
          <small>{slot === 0 ? 'Host game' : 'Guest game'}</small>
          <em>{slot === 0 ? 'the world you play in' : 'what crashes into it'}</em>
        </span>
      )}
    </button>
  );
}

export function Home({ ctx, query }: { ctx: Ctx; query: string }) {
  const [filter, setFilter] = useState<Filter>('playable');
  const privacy = usePrivacy();
  const [a, b] = ctx.pair;

  const pairHits = useMemo(() => {
    if (!a && !b) return [];
    return ctx.catalog.filter((m) => {
      const sides = [m.host, m.guest];
      return (!a || sides.includes(a)) && (!b || sides.includes(b));
    });
  }, [a, b]);

  const q = query.trim().toLowerCase();
  const list = ctx.catalog.filter((m) => {
    if (q) return [m.name, m.tagline, m.by.name, gameName(m.host), GAME[m.guest ?? '']?.name ?? m.guest].join(' ').toLowerCase().includes(q);
    if (filter === 'playable') return missing(ctx, m).length === 0;
    if (filter === 'crossover') return m.kind === 'passthrough';
    return true;
  });
  const oneAway = ctx.catalog.filter((m) => missing(ctx, m).length === 1);
  const playableCount = ctx.catalog.filter((m) => missing(ctx, m).length === 0).length;
  const live = ctx.agents.filter((x) => x.status === 'building' || x.live).slice(0, 8);

  const bgA = a && ctx.scan?.games.find((x) => x.canon === a);

  return (
    <div className="page">
      <section className="board">
        <div className="board-bg" aria-hidden>
          {a && <GameArt id={a} name={a} src={bgA ? bgA.art : undefined} local={bgA ? bgA.heroLocal ?? bgA.artLocal : undefined} />}
        </div>
        <div className="board-copy">
          <span className="eyebrow">Mix board</span>
          <h1>
            Two games you own.<br />
            <span className="chrome">One new game.</span>
          </h1>
          <p>
            Pick a host and a guest. You get every mashup that already exists for the pair, ready in one click. If nobody made it yet, an agent builds it for you.
          </p>
          {!ctx.scan ? (
            <div className="scanline">Looking for your games…</div>
          ) : (
            <div className="scanline done">
              <b>{ctx.scan.games.length}</b> games on this PC · <b>{playableCount}</b> mashups playable right now · scanned in {ctx.scan.millis} ms
            </div>
          )}
        </div>
        <div className="board-slots">
          <Slot ctx={ctx} slot={0} />
          <button className="x" onClick={() => ctx.setPair([b, a])} title="Swap host and guest">
            <span>×</span>
            <Icon name="swap" size={14} />
          </button>
          <Slot ctx={ctx} slot={1} />
        </div>
        {(a || b) && (
          <div className="board-result">
            {pairHits.length > 0 ? (
              <>
                <span className="eyebrow">{pairHits.length} for {[a, b].filter(Boolean).map((x) => gameName(x!)).join(' × ')}</span>
                <div className="pair-list">
                  {pairHits.map((m) => (
                    <button key={m.id} className="pair-hit" onClick={() => ctx.open(m)}>
                      <b>{m.name}</b>
                      <span>{m.kind === 'passthrough' ? 'Real crossover' : 'Mashup'} · {m.by.name}</span>
                    </button>
                  ))}
                  {a && b && (
                    <button className="pair-hit pair-build" onClick={() => ctx.go('build')}>
                      <b>Build your own take</b>
                      <span>Agent · about 45 min</span>
                    </button>
                  )}
                </div>
              </>
            ) : (
              <div className="pair-none">
                <span>Nobody has made {[a, b].filter(Boolean).map((x) => gameName(x!)).join(' × ')} yet.</span>
                <button className="act act-get" onClick={() => ctx.go('build')}>
                  <Icon name="build" size={15} /> Build it
                </button>
              </div>
            )}
          </div>
        )}
      </section>

      <Section
        title={q ? `Results for “${query}”` : 'Mashups'}
        sub={q ? undefined : filter === 'playable' ? 'You own every game these need. One click to play.' : undefined}
        aside={
          !q && (
            <div className="seg">
              {([['playable', `Playable now · ${playableCount}`], ['crossover', 'Real crossovers'], ['all', 'Everything']] as const).map(([f, l]) => (
                <button key={f} className={filter === f ? 'on' : ''} onClick={() => setFilter(f)}>{l}</button>
              ))}
            </div>
          )
        }
      >
        {list.length ? (
          <div className="grid">{list.map((m, i) => <Card key={m.id} ctx={ctx} m={m} i={i} />)}</div>
        ) : (
          <div className="empty">Nothing here yet. Try “Everything”, or build it.</div>
        )}
      </Section>

      {live.length > 0 && !q && (
        <Section title="Building right now" sub="Agents on the SIGF launchpad. Finished builds land here with an Install button." aside={<span className="live-pill"><Icon name="live" size={10} /> live from sigf.ai</span>}>
          <div className="rail-row">
            {live.map((x) => (
              <a key={x.ticker} className="agent" href={`https://sigf.ai/agent/${x.ticker}`} target="_blank" rel="noreferrer">
                <div className="agent-img">{imageOk(x.image, privacy) ? <img src={x.image} alt="" /> : <span>${x.ticker}</span>}</div>
                <div>
                  <b>{x.name}</b>
                  <span>{gameName(x.host)} × {x.guest}</span>
                  <em className={x.status === 'building' ? 'on' : ''}>{x.status === 'building' ? 'building' : x.status}</em>
                </div>
              </a>
            ))}
          </div>
        </Section>
      )}

      {oneAway.length > 0 && !q && (
        <Section title="One game away" sub="You own all but one game for these.">
          <div className="grid grid-sm">{oneAway.map((m, i) => <Card key={m.id} ctx={ctx} m={m} i={i} />)}</div>
        </Section>
      )}
    </div>
  );
}
