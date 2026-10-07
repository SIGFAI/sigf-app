import { useState } from 'react';
import type { Ctx } from '../App';
import { GAME } from '../data/games';
import { openUrl } from '../lib/api';
import { GameArt, Icon } from '../ui';
import { t, tx } from '../i18n';

/** The submission form on the site (opens in the browser). */
const SUBMIT_URL = 'https://sigf.ai/submit';

const IDEAS = ['build.idea1', 'build.idea2', 'build.idea3', 'build.idea4'] as const;
const FACTS = [['build.fact1', 'build.fact1b'], ['build.fact2', 'build.fact2b'], ['build.fact3', 'build.fact3b'], ['build.fact4', 'build.fact4b']] as const;

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
        <span className="eyebrow">{t('build.eyebrow')}</span>
        <h1>{t('build.title1')} <span className="chrome">{t('build.title2')}</span></h1>
        <p>{t('build.lede')}</p>
      </section>

      <div className="build-grid">
        <div className="build-step">
          <span className="num">01</span>
          <h3>{t('build.step1')}</h3>
          <div className="build-pair">
            {[0, 1].map((s) => {
              const id = ctx.pair[s];
              return (
                <button key={s} className="mini-slot" onClick={() => ctx.pick(s as 0 | 1)}>
                  {id ? <GameArt id={id} name={GAME[id]?.name ?? id} /> : <Icon name="plus" size={20} />}
                  <span>{id ? GAME[id]?.short : s === 0 ? t('mix.host') : t('mix.guest')}</span>
                </button>
              );
            })}
          </div>
        </div>

        <div className="build-step">
          <span className="num">02</span>
          <h3>{t('build.step2')}</h3>
          <textarea value={twist} onChange={(e) => setTwist(e.target.value)} placeholder={t('build.twistPlaceholder')} rows={4} />
          <div className="ideas">
            {IDEAS.map((i) => <button key={i} onClick={() => setTwist(t(i))}>{t(i)}</button>)}
          </div>
        </div>

        <div className="build-step">
          <span className="num">03</span>
          <h3>{t('build.step3')}</h3>
          <ul className="build-facts">
            {FACTS.map(([f, b]) => <li key={f}>{tx(f, { b: <b>{t(b)}</b> })}</li>)}
          </ul>
          <button className="act act-big act-get" disabled={!ready} onClick={launch}>
            <Icon name="build" size={16} /> {t('build.launch')}
          </button>
          {!ready && <small className="muted">{t('build.notReady')}</small>}
        </div>
      </div>

      <section className="build-submit">
        <div>
          <h3>{t('build.submitTitle')}</h3>
          <p>{t('build.submitSub')}</p>
        </div>
        <button className="act act-ghost" onClick={() => openUrl(SUBMIT_URL)}>
          {t('build.submit')} <Icon name="ext" size={12} />
        </button>
      </section>
    </div>
  );
}
