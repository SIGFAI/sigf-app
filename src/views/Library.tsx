import { useState } from 'react';
import type { Ctx } from '../App';
import { launchGame } from '../lib/api';
import { GameArt, Icon, STORE_LABEL } from '../ui';

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
            <h2>Your games</h2>
            <p>Found on this PC. Nothing to link, no sign-in: the app reads what your stores already installed.</p>
          </div>
          <div className="seg">
            {['all', ...(ctx.scan?.stores ?? [])].map((s) => (
              <button key={s} className={store === s ? 'on' : ''} onClick={() => setStore(s)}>{s === 'all' ? 'All' : STORE_LABEL[s]}</button>
            ))}
          </div>
        </header>
        {!ctx.scan && <div className="empty">Scanning Steam, Epic, Ubisoft, GOG and Minecraft…</div>}
        <div className="shelf">
          {games.map((g, i) => {
            const n = g.canon ? ctx.catalog.filter((m) => m.needs.includes(g.canon!) || m.guest === g.canon).length : 0;
            return (
              <div key={g.key} className="tile" style={{ ['--i' as string]: i }}>
                <GameArt id={g.canon} name={g.name} src={g.art} wide={g.artWide} local={g.artLocal} heroLocal={g.heroLocal} wideLocal={g.wideLocal} />
                <div className="tile-over">
                  <span className={`store-badge s-${g.store}`}>{STORE_LABEL[g.store]}</span>
                  {n > 0 && <span className="mods-badge">{n} mashup{n > 1 ? 's' : ''}</span>}
                  <div className="tile-actions">
                    {g.canon && (
                      <button
                        onClick={() => {
                          ctx.setPair([g.canon!, ctx.pair[1] === g.canon ? null : ctx.pair[1]]);
                          ctx.go('mix');
                        }}
                      >
                        Mix as host
                      </button>
                    )}
                    {g.launch && (
                      <button className="ghost" onClick={() => launchGame(g.launch!)} title="Launch vanilla">
                        <Icon name="play" size={12} />
                      </button>
                    )}
                  </div>
                </div>
                <div className="tile-name">
                  {g.name}
                  {g.build && <small title={g.build}>build {shortBuild(g.build)}</small>}
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
              <h2>Minecraft launchers</h2>
              <p>Minecraft mashups install as a new instance in your launcher. Your worlds and accounts stay where they are.</p>
            </div>
          </header>
          <div className="launchers">
            {mc.map((l) => (
              <div key={l.kind} className="launcher">
                <b>{l.kind === 'prism' ? 'Prism Launcher' : l.kind === 'modrinth' ? 'Modrinth App' : 'Official launcher'}</b>
                <span>{l.instances.length} instance{l.instances.length === 1 ? '' : 's'}{l.kind === 'prism' ? ' · used for installs' : ''}</span>
              </div>
            ))}
          </div>
        </section>
      )}
    </div>
  );
}
