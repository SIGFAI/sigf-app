import { useState } from 'react';
import type { Ctx } from '../App';
import { GAME } from '../data/games';
import { openUrl } from '../lib/api';
import { GameArt, Icon } from '../ui';

const IDEAS = ['The guest’s boss invades every 10 minutes', 'Swap every weapon for the guest’s signature item', 'Score attack: the guest’s rules, the host’s world', 'Co-op wave survival with the guest’s enemies'];

export function Build({ ctx }: { ctx: Ctx }) {
  const [a, b] = ctx.pair;
  const [twist, setTwist] = useState('');
  const ready = a && b && twist.trim().length > 8;

  const launch = () => {
    const q = new URLSearchParams({ host: a!, guest: GAME[b!]?.name ?? b!, twist: twist.trim(), from: 'app' });
    openUrl(`https://sigf.ai/studio/launch?${q}`);
  };

  return (
    <div className="page build">
      <section className="build-hero">
        <span className="eyebrow">Build anything</span>
        <h1>Describe it. <span className="chrome">An agent builds it.</span></h1>
        <p>An AI agent gets its own machine with the host game, writes the mod live on stream, tests it, and ships it to your library. You can watch every line.</p>
      </section>

      <div className="build-grid">
        <div className="build-step">
          <span className="num">01</span>
          <h3>Pick the pair</h3>
          <div className="build-pair">
            {[0, 1].map((s) => {
              const id = ctx.pair[s];
              return (
                <button key={s} className="mini-slot" onClick={() => ctx.pick(s as 0 | 1)}>
                  {id ? <GameArt id={id} name={GAME[id]?.name ?? id} /> : <Icon name="plus" size={20} />}
                  <span>{id ? GAME[id]?.short : s === 0 ? 'Host' : 'Guest'}</span>
                </button>
              );
            })}
          </div>
        </div>

        <div className="build-step">
          <span className="num">02</span>
          <h3>The twist</h3>
          <textarea value={twist} onChange={(e) => setTwist(e.target.value)} placeholder="What should happen when these two games meet?" rows={4} />
          <div className="ideas">
            {IDEAS.map((i) => <button key={i} onClick={() => setTwist(i)}>{i}</button>)}
          </div>
        </div>

        <div className="build-step">
          <span className="num">03</span>
          <h3>Launch the agent</h3>
          <ul className="build-facts">
            <li><b>~45 min</b> for a first playable build</li>
            <li><b>Live</b> stream of the agent’s screen</li>
            <li><b>Public</b> source on GitHub, MIT</li>
            <li><b>Lands here</b> with an Install button when done</li>
          </ul>
          <button className="act act-big act-get" disabled={!ready} onClick={launch}>
            <Icon name="build" size={16} /> Launch on SIGF
          </button>
          {!ready && <small className="muted">Pick both games and write a twist first.</small>}
        </div>
      </div>
    </div>
  );
}
