import { useCallback, useEffect, useRef, useState } from 'react';
import { getText, openUrl, type Agent } from '../lib/api';
import { attach, drop, keepLive, loadHls } from '../lib/hls';
import { imageOk, usePrivacy } from '../lib/privacy';
import { Avatar, Icon, MashupCover } from '../ui';
import { gameName } from './shared';
import { t } from '../i18n';

const SITE = 'https://sigf.ai';
const LIST_MS = 15_000;    // who is live, read again while the tab is open
const FRAME_MS = 4_000;    // each card's latest picture
const RETRY_MS = 3_000;    // a stalled or broken player is rebuilt after this

/** One live build: the main stream's, or a launchpad agent's. `play`: its HLS playlist, when it has video. A null
 *  `title` or `by` is the main stream's own (said in the player's language, `between`: between two builds). */
type Stream = {
  id: string; url: string; host: string | null; guest: string | null; title: string | null; between?: boolean; by: string | null; avatar?: string;
  tag: 'main' | 'crossover' | 'launchpad'; frame: string | null; play: string | null; since: string | null; step: string | null; starting: boolean;
};
type State = { building: { takenAt: string | null; mix: { host: string; guest: string | null } | null; mod: { nom?: string } | null } | null; frame: string };
/** sigf.ai's /api/live: the main stream's live video (HLS), relayed by sigf.ai under /hls/ (`proxyUrl`). */
type MainLive = { status: 'ACTIVE' | 'ENDED' | 'NONE'; proxyUrl: string | null };

/** A playlist the player may load: on sigf.ai only (its /hls/ relay or its /live/ agent streams), as the CSP allows. */
function playable(u: string | null | undefined): string | null {
  if (!u) return null;
  try {
    const url = new URL(u, SITE);
    return url.origin === SITE && /^\/(hls|live)\/[^?#]+\.m3u8$/.test(url.pathname) ? url.href : null;
  } catch {
    return null;
  }
}

/** Who is live now; null when sigf.ai did not answer (the list on screen stays, and an open player keeps playing). */
async function readStreams(): Promise<Stream[] | null> {
  const [st, main, agents] = await Promise.all([
    getText(`${SITE}/api/state`).then((t) => JSON.parse(t) as State, () => null),
    getText(`${SITE}/api/live`).then((t) => JSON.parse(t) as MainLive, () => null),
    getText(`${SITE}/api/studio/agents`).then((t) => JSON.parse(t) as Agent[], () => null),
  ]);
  if (!st || !main || !agents) return null;
  const out: Stream[] = [];
  const b = st.building;
  const mainPlay = main.status === 'ACTIVE' ? playable(main.proxyUrl) : null;
  // The main stream: while a build runs, and between builds too when its live is on (it shows the next round).
  if (b?.mix || mainPlay) {
    out.push({
      id: 'main', url: SITE, host: b?.mix?.host ?? null, guest: b?.mix?.guest ?? null, title: b?.mix ? b.mod?.nom ?? null : null, between: !b?.mix,
      by: null, avatar: `${SITE}/favicon.svg`, tag: 'main', frame: st.frame ? new URL(st.frame, SITE).href : null,
      play: mainPlay, since: b?.mix ? b.takenAt : null, step: null, starting: false,
    });
  }
  for (const a of agents) {
    const m = a.machine;
    if (m?.state !== 'building' && m?.state !== 'starting') continue;
    const mix = a.mixNow ?? { host: a.host, guest: a.guest, crossover: a.crossover };
    const p = m.progress;
    out.push({
      id: a.ticker, url: `${SITE}/agent/${encodeURIComponent(a.ticker)}`, host: mix.host, guest: mix.guest, title: a.name, by: `$${a.ticker}`,
      avatar: a.image ? new URL(a.image, SITE).href : undefined, tag: mix.crossover ? 'crossover' : 'launchpad',
      frame: m.frame ? new URL(m.frame, SITE).href : null, play: m.state === 'building' ? playable(m.stream) : null, since: null, starting: m.state === 'starting',
      step: p ? `${p.done}/${p.total}${p.current ? ` · ${p.current}` : ''}` : null,
    });
  }
  return out;
}

const mixLabel = (s: Stream) => (s.host ? <>{gameName(s.host)}{s.guest && <> × {gameName(s.guest)}</>}</> : t('live.mainStream'));
const titleOf = (s: Stream) => s.title ?? (s.between ? t('live.between') : t('live.mainBuild'));
const byOf = (s: Stream) => s.by ?? t('live.mainStream');
const TAG = { main: 'live.tagMain', crossover: 'live.tagCrossover', launchpad: 'live.tagLaunchpad' } as const;

/** Elapsed time since `iso`, ticking every second. */
function Clock({ since }: { since: string }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, []);
  const s = Math.max(0, Math.floor((now - Date.parse(since)) / 1000));
  const pad = (n: number) => String(n).padStart(2, '0');
  return <span className="lv-time">{Math.floor(s / 3600)}:{pad(Math.floor(s / 60) % 60)}:{pad(s % 60)}</span>;
}

/** The build's latest picture, swapped in only once the next one has loaded (no flash); the pair's art until then. */
function Frame({ s, tick }: { s: Stream; tick: number }) {
  const privacy = usePrivacy();
  const [shown, setShown] = useState<string | null>(null);
  useEffect(() => {
    if (!s.frame || !imageOk(s.frame, privacy)) return;
    const next = new Image();
    let live = true;
    next.onload = () => live && setShown(next.src);
    next.src = `${s.frame}${s.frame.includes('?') ? '&' : '?'}t=${tick}`;
    return () => { live = false; };
  }, [s.frame, tick, privacy]);
  return (
    <>
      {s.host ? <MashupCover host={s.host} guest={s.guest ?? undefined} className="lv-art" /> : <i className="lv-art lv-art-plain" />}
      {shown && <img className="lv-frame" src={shown} alt="" draggable={false} />}
    </>
  );
}

/**
 * The stream inside the app, like sigf.ai's player: hls.js, muted autoplay, the latest frame under it and "Starting"
 * until the video really plays, rebuilt when it stalls. The same playlist keeps its player across list refreshes;
 * closing the view (Back, Esc, another tab) destroys it, so nothing more is downloaded.
 */
function Player({ s, ended, tick, onClose }: { s: Stream; ended: boolean; tick: number; onClose: () => void }) {
  const video = useRef<HTMLVideoElement>(null);
  const [gen, setGen] = useState(0);            // bumped to rebuild a stalled or broken player
  const [playing, setPlaying] = useState(false);
  const [muted, setMuted] = useState(true);
  const [unsupported, setUnsupported] = useState(false);
  const src = ended ? null : s.play;

  useEffect(() => {
    const k = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    window.addEventListener('keydown', k);
    return () => window.removeEventListener('keydown', k);
  }, [onClose]);

  useEffect(() => {
    const v = video.current;
    setPlaying(false);
    if (!v || !src) return;
    let live = true, retry: ReturnType<typeof setTimeout> | undefined, unwatch: (() => void) | undefined;
    let hls: ReturnType<typeof attach> | null = null;
    const onPlaying = () => live && setPlaying(true);
    v.addEventListener('playing', onPlaying);
    void loadHls().then((H) => {
      if (!live) return;
      if (!H) { setUnsupported(true); return; }
      hls = attach(H, v, src);
      unwatch = keepLive(v, hls, H, () => {
        drop(v, hls);
        hls = null;
        if (live) { setPlaying(false); retry = setTimeout(() => live && setGen((g) => g + 1), RETRY_MS); }
      });
      v.play().catch(() => {});
    });
    return () => {
      live = false;
      clearTimeout(retry);
      unwatch?.();
      v.removeEventListener('playing', onPlaying);
      drop(v, hls);
    };
  }, [src, gen]);

  useEffect(() => {
    if (video.current) video.current.muted = muted;
  }, [muted]);

  const badge = ended ? t('live.ended') : playing ? t('live.live') : t('live.starting');
  const note = ended
    ? t('live.endedNote')
    : !src
      ? s.id === 'main'
        ? t('live.noVideoMain')
        : t('live.noVideoAgent')
      : unsupported ? t('live.unsupported') : null;

  return (
    <div className="page lp">
      <div className="lp-bar">
        <button className="act act-ghost" onClick={onClose}><Icon name="back" size={14} /> {t('nav.live')}</button>
        <a className="pv-link" onClick={() => void openUrl(s.url)} title={s.url.replace('https://', '')}>{t('live.openSite')} <Icon name="ext" size={11} /></a>
      </div>
      <div className="lp-stage">
        {!playing && <Frame s={s} tick={tick} />}
        <video ref={video} className={`lp-video ${playing ? 'on' : ''}`} muted autoPlay playsInline disablePictureInPicture aria-label={t('live.viewOf', { title: titleOf(s) })} />
        <span className={`lv-badge ${playing ? '' : 'wait'}`}>{badge}</span>
        {s.since && !ended && <Clock since={s.since} />}
        {note && <span className="lp-note">{note}</span>}
        {src && playing && (
          <button className="lp-sound" onClick={() => setMuted((m) => !m)} aria-label={muted ? t('live.soundOn') : t('live.mute')}>
            <Icon name={muted ? 'muted' : 'sound'} size={16} /> {muted ? t('live.soundOn') : t('live.mute')}
          </button>
        )}
      </div>
      <div className="lp-meta">
        {s.avatar ? <Avatar src={s.avatar} size={48} /> : <i className="lv-noav lp-noav" />}
        <div>
          <span className="eyebrow">{t(TAG[s.tag])}</span>
          <h2>{titleOf(s)}</h2>
          <b>{mixLabel(s)}</b>
          <span>{byOf(s)}</span>
        </div>
        {s.step && !ended && <span className="lp-step"><small>{t('live.step')}</small>{s.step}</span>}
      </div>
    </div>
  );
}

export function Live() {
  const [streams, setStreams] = useState<Stream[] | null>(null);
  const [tick, setTick] = useState(Date.now());
  const [open, setOpen] = useState<Stream | null>(null);
  // Stable, so the player's Escape listener is not subscribed again on every tick.
  const close = useCallback(() => setOpen(null), []);
  useEffect(() => {
    let live = true;
    const read = () => !document.hidden && readStreams().then((l) => live && setStreams((was) => l ?? was ?? []));
    read();
    const list = setInterval(read, LIST_MS);
    const frames = setInterval(() => !document.hidden && setTick(Date.now()), FRAME_MS);
    return () => {
      live = false;
      clearInterval(list);
      clearInterval(frames);
    };
  }, []);

  if (open) {
    // The open stream as the latest list has it; gone from the list: its build ended (the last one known stays on screen).
    const now = streams?.find((x) => x.id === open.id);
    return <Player s={now ?? open} ended={!!streams && !now} tick={tick} onClose={close} />;
  }

  return (
    <div className="page">
      <section className="section">
        <header>
          <div>
            <h2>{t('live.title')}</h2>
            <p>{t('live.sub')}</p>
          </div>
          {streams && streams.length > 0 && <span className="live-pill"><Icon name="live" size={10} /> {t('live.count', { count: streams.length })}</span>}
        </header>
        {!streams && <div className="empty">{t('live.looking')}</div>}
        {streams?.length === 0 && <div className="empty">{t('live.none')}</div>}
        <div className="lv-grid">
          {streams?.map((s, i) => (
            <button key={s.id} className={`lv-card ${s.id === 'main' ? 'lv-main' : ''}`} style={{ ['--i' as string]: i }} onClick={() => setOpen(s)} title={t('live.watchTitle', { title: titleOf(s) })}>
              <div className="lv-thumb">
                <Frame s={s} tick={tick} />
                <span className={`lv-badge ${s.starting ? 'wait' : ''}`}>{s.starting ? t('live.starting') : t('live.live')}</span>
                {s.since && <Clock since={s.since} />}
                {s.step && <span className="lv-step">{s.step}</span>}
                <span className="lv-watch"><Icon name="play" size={14} /> {t('live.watch')}</span>
              </div>
              <div className="lv-meta">
                {s.avatar ? <Avatar src={s.avatar} size={36} /> : <i className="lv-noav" />}
                <div>
                  <b>{mixLabel(s)}</b>
                  <span>{titleOf(s)} · {byOf(s)}</span>
                  <em className="chip">{t(TAG[s.tag])}</em>
                </div>
              </div>
            </button>
          ))}
        </div>
      </section>
    </div>
  );
}
