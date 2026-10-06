import { useEffect, useState } from 'react';
import { getText, openUrl, type Agent } from '../lib/api';
import { imageOk, usePrivacy } from '../lib/privacy';
import { Avatar, Icon, MashupCover } from '../ui';
import { gameName } from './shared';

const SITE = 'https://sigf.ai';
const LIST_MS = 15_000;    // who is live, read again while the tab is open
const FRAME_MS = 4_000;    // each card's latest picture

/** One live build: the main stream's, or a launchpad agent's. */
type Stream = {
  id: string; url: string; host: string; guest: string | null; title: string; by: string; avatar?: string;
  tag: string; frame: string | null; since: string | null; step: string | null; starting: boolean;
};
type State = { building: { takenAt: string | null; mix: { host: string; guest: string | null } | null; mod: { nom?: string } | null } | null; frame: string };

async function readStreams(): Promise<Stream[]> {
  const [st, agents] = await Promise.all([
    getText(`${SITE}/api/state`).then((t) => JSON.parse(t) as State, () => null),
    getText(`${SITE}/api/studio/agents`).then((t) => JSON.parse(t) as Agent[], () => [] as Agent[]),
  ]);
  const out: Stream[] = [];
  const b = st?.building;
  if (b?.mix) {
    out.push({
      id: 'main', url: SITE, host: b.mix.host, guest: b.mix.guest, title: b.mod?.nom ?? 'The main build', by: 'SIGF main stream', avatar: `${SITE}/favicon.svg`,
      tag: 'Main stream', frame: st!.frame ? new URL(st!.frame, SITE).href : null, since: b.takenAt, step: null, starting: false,
    });
  }
  for (const a of agents) {
    const m = a.machine;
    if (m?.state !== 'building' && m?.state !== 'starting') continue;
    const mix = a.mixNow ?? { host: a.host, guest: a.guest, crossover: a.crossover };
    const p = m.progress;
    out.push({
      id: a.ticker, url: `${SITE}/agent/${encodeURIComponent(a.ticker)}`, host: mix.host, guest: mix.guest, title: a.name, by: `$${a.ticker}`,
      avatar: a.image ? new URL(a.image, SITE).href : undefined, tag: mix.crossover ? 'Crossover' : 'Launchpad',
      frame: m.frame ? new URL(m.frame, SITE).href : null, since: null, starting: m.state === 'starting',
      step: p ? `${p.done}/${p.total}${p.current ? ` · ${p.current}` : ''}` : null,
    });
  }
  return out;
}

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
      <MashupCover host={s.host} guest={s.guest ?? undefined} className="lv-art" />
      {shown && <img className="lv-frame" src={shown} alt="" draggable={false} />}
    </>
  );
}

export function Live() {
  const [streams, setStreams] = useState<Stream[] | null>(null);
  const [tick, setTick] = useState(Date.now());
  useEffect(() => {
    let live = true;
    const read = () => !document.hidden && readStreams().then((l) => live && setStreams(l));
    read();
    const list = setInterval(read, LIST_MS);
    const frames = setInterval(() => !document.hidden && setTick(Date.now()), FRAME_MS);
    return () => {
      live = false;
      clearInterval(list);
      clearInterval(frames);
    };
  }, []);

  return (
    <div className="page">
      <section className="section">
        <header>
          <div>
            <h2>Live now</h2>
            <p>Every game an AI is modding right now on SIGF: the main stream's build and each launchpad agent. Click one to watch it on sigf.ai.</p>
          </div>
          {streams && streams.length > 0 && <span className="live-pill"><Icon name="live" size={10} /> {streams.length} live</span>}
        </header>
        {!streams && <div className="empty">Looking for live builds…</div>}
        {streams?.length === 0 && <div className="empty">No AI is building live right now. The next build starts soon.</div>}
        <div className="lv-grid">
          {streams?.map((s, i) => (
            <button key={s.id} className={`lv-card ${s.id === 'main' ? 'lv-main' : ''}`} style={{ ['--i' as string]: i }} onClick={() => void openUrl(s.url)} title={`Watch on ${s.url.replace('https://', '')}`}>
              <div className="lv-thumb">
                <Frame s={s} tick={tick} />
                <span className={`lv-badge ${s.starting ? 'wait' : ''}`}>{s.starting ? 'Starting' : 'Live'}</span>
                {s.since && <Clock since={s.since} />}
                {s.step && <span className="lv-step">{s.step}</span>}
                <span className="lv-watch"><Icon name="ext" size={14} /> Watch on sigf.ai</span>
              </div>
              <div className="lv-meta">
                {s.avatar ? <Avatar src={s.avatar} size={36} /> : <i className="lv-noav" />}
                <div>
                  <b>{gameName(s.host)}{s.guest && <> × {gameName(s.guest)}</>}</b>
                  <span>{s.title} · {s.by}</span>
                  <em className="chip">{s.tag}</em>
                </div>
              </div>
            </button>
          ))}
        </div>
      </section>
    </div>
  );
}
