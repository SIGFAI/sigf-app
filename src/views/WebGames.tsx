import { useEffect, useRef, useState } from 'react';
import { WEB_GAMES, type WebGame } from '../data/webgames';
import { inTauri, onWebFullscreen, openUrl, webClose, webFrame, webFullscreen, webOpen, type WebFrame } from '../lib/api';
import { GameArt, Icon } from '../ui';
import { t } from '../i18n';

const frameOf = (el: HTMLElement): WebFrame => {
  const r = el.getBoundingClientRect();
  return { x: r.left, y: r.top, w: r.width, h: r.height };
};

/** The game, in the frame below the bar. In the app it is a child webview laid over the frame (sites that refuse
 *  iframes or need cross-origin isolation still run); in a plain browser (vite dev) an iframe stands in. */
function Player({ game, onBack }: { game: WebGame; onBack: () => void }) {
  const frame = useRef<HTMLDivElement>(null);
  const [full, setFull] = useState(false);

  useEffect(() => {
    if (!inTauri || !frame.current) return;
    const el = frame.current;
    webOpen(game.url, frameOf(el)).catch(console.error);
    const ro = new ResizeObserver(() => webFrame(frameOf(el)).catch(() => {}));
    ro.observe(el);
    const move = () => webFrame(frameOf(el)).catch(() => {});
    window.addEventListener('resize', move);
    const off = onWebFullscreen((on) => {
      setFull(on);
      if (!on) requestAnimationFrame(move);
    });
    return () => {
      ro.disconnect();
      window.removeEventListener('resize', move);
      off.then((f) => f());
      webClose().catch(() => {});
    };
  }, [game.url]);

  return (
    <div className="web-player">
      <div className="web-bar">
        <button className="act act-ghost" onClick={onBack}><Icon name="back" size={14} /> {t('web.back')}</button>
        <b>{game.name}</b>
        <small>{new URL(game.url).host}</small>
        <span className="web-hint">{t('web.fullHint')}</span>
        <button className="act act-ghost" onClick={() => openUrl(game.url)} title={t('web.openBrowser')}><Icon name="ext" size={14} /></button>
        <button className="act act-get" onClick={() => webFullscreen(!full).catch(console.error)} disabled={!inTauri}><Icon name="max" size={12} /> {t('web.fullscreen')}</button>
      </div>
      <div className="web-frame" ref={frame}>
        {!inTauri && <iframe src={game.url} title={game.name} allow="fullscreen; autoplay; gamepad; pointer-lock" />}
      </div>
    </div>
  );
}

export function WebGames() {
  const [q, setQ] = useState('');
  const [playing, setPlaying] = useState<WebGame | null>(null);
  const games = WEB_GAMES.filter((g) => g.name.toLowerCase().includes(q.trim().toLowerCase()));

  if (playing) return <Player game={playing} onBack={() => setPlaying(null)} />;

  return (
    <div className="page">
      <section className="section">
        <header>
          <div>
            <h2>{t('web.title')}</h2>
            <p>{t('web.sub')}</p>
          </div>
          <input className="web-search" value={q} onChange={(e) => setQ(e.target.value)} placeholder={t('web.filter')} />
        </header>
        <div className="shelf">
          {games.map((g, i) => (
            <div key={g.id} className="tile web-tile" style={{ ['--i' as string]: i }} onClick={() => setPlaying(g)}>
              <GameArt name={g.art ?? g.name} src={`/art/web/${g.id}.webp`} />
              <div className="tile-over">
                {g.note && <span className="mods-badge">{g.note}</span>}
                <div className="tile-actions">
                  <button><Icon name="play" size={12} /> {t('web.play')}</button>
                </div>
              </div>
              <div className="tile-name">
                {g.name}
                <small>{new URL(g.url).host}</small>
              </div>
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}
