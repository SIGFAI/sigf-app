import { useState } from 'react';
import type { Ctx } from '../App';
import { launchGame } from '../lib/api';
import { GameArt, Icon, STORE_LABEL } from '../ui';
import { t } from '../i18n';

/** Epic build strings run to 40+ chars (`++Fortnite+Release-42.30-CL-58557680-Windows`): keep the version part. */
const shortBuild = (b: string) => {
  const rel = b.match(/Release-([\d.]+)/);
  if (rel) return rel[1];
  return b.length > 18 ? `${b.slice(0, 16)}…` : b;
};

export function Library({ ctx }: { ctx: Ctx }) {
  const [store, setStore] = useState<string>('all');
  const games = (ctx.scan?.games ?? []).filter((g) => store === 'all' || g.store === store);
  const mc = ctx.scan?.launchers ?? [];

  return (
    <div className="page">
      <section className="section">
        <header>
          <div>
            <h2>{t('lib.title')}</h2>
            <p>{t('lib.sub')}</p>
          </div>
          <div className="seg">
            {['all', ...(ctx.scan?.stores ?? [])].map((s) => (
              <button key={s} className={store === s ? 'on' : ''} onClick={() => setStore(s)}>{s === 'all' ? t('lib.all') : STORE_LABEL[s]}</button>
            ))}
          </div>
        </header>
        {!ctx.scan && <div className="empty">{t('lib.scanning')}</div>}
        <div className="shelf">
          {games.map((g, i) => {
            const n = g.canon ? ctx.catalog.filter((m) => m.needs.includes(g.canon!) || m.guest === g.canon).length : 0;
            return (
              <div key={g.key} className="tile" style={{ ['--i' as string]: i }}>
                <GameArt id={g.canon} name={g.name} src={g.art} wide={g.artWide} local={g.artLocal} heroLocal={g.heroLocal} wideLocal={g.wideLocal} />
                <div className="tile-over">
                  <span className={`store-badge s-${g.store}`}>{STORE_LABEL[g.store]}</span>
                  {n > 0 && <span className="mods-badge">{t('lib.mashups', { count: n })}</span>}
                  <div className="tile-actions">
                    {g.canon && (
                      <button
                        onClick={() => {
                          ctx.setPair([g.canon!, ctx.pair[1] === g.canon ? null : ctx.pair[1]]);
                          ctx.go('mix');
                        }}
                      >
                        {t('lib.mixAsHost')}
                      </button>
                    )}
                    {g.launch && (
                      <button className="ghost" onClick={() => launchGame(g.launch!)} title={t('lib.launchVanilla')} aria-label={t('lib.launchVanilla')}>
                        <Icon name="play" size={12} />
                      </button>
                    )}
                  </div>
                </div>
                <div className="tile-name">
                  {g.name}
                  {g.build && <small title={g.build}>{t('lib.build', { build: shortBuild(g.build) })}</small>}
                </div>
              </div>
            );
          })}
        </div>
      </section>

      {mc.length > 0 && (
        <section className="section">
          <header>
            <div>
              <h2>{t('lib.launchers')}</h2>
              <p>{t('lib.launchersSub')}</p>
            </div>
          </header>
          <div className="launchers">
            {mc.map((l) => (
              <div key={l.kind} className="launcher">
                <b>{l.kind === 'prism' ? 'Prism Launcher' : l.kind === 'modrinth' ? 'Modrinth App' : t('lib.official')}</b>
                <span>{t('lib.instances', { count: l.instances.length })}{l.kind === 'prism' ? ` · ${t('lib.usedForInstalls')}` : ''}</span>
              </div>
            ))}
          </div>
        </section>
      )}
    </div>
  );
}
