import { useEffect, useState, type CSSProperties, type ReactNode } from 'react';
import { GAME, GAMES, art } from './data/games';
import { localSrc, steamLookup, type SteamArt } from './lib/api';
import { imageOk, usePrivacy } from './lib/privacy';

const hash = (s: string) => [...s].reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 7);

/** A game's portrait art. Order: Steam's own art cache on disk (newer apps only serve art under hashed CDN paths), the
 *  store's art, the same title found on Steam by name, then our generated look-alike key art (public/art), and last a
 *  flat generated tile. The privacy choices decide which may load (docs/PRIVACY.md): with pictures off, only local art. */
export function GameArt({ id, name, src, wide, local, heroLocal, wideLocal, hero = false, className = '', style }: {
  id?: string | null; name: string; src?: string; wide?: string;
  local?: string; heroLocal?: string; wideLocal?: string;
  hero?: boolean; className?: string; style?: CSSProperties;
}) {
  // A store we do not map by id (Ubisoft, ...) still gets the game's art when its name is a canonical game's name.
  const cid = id ?? GAMES.find((g) => g.name.toLowerCase() === name.toLowerCase())?.id;
  const canon = cid ? art(cid) : {};
  const privacy = usePrivacy();
  const [found, setFound] = useState<SteamArt | null>(null);
  const base = hero
    ? [localSrc(heroLocal), canon.hero, canon.wide, localSrc(wideLocal), wide]
    : [localSrc(local), src, canon.tall, localSrc(wideLocal), wide, canon.wide];
  const more = found ? (hero ? [found.hero, found.wide] : [found.art, found.wide]) : [];
  const gen = canon.gen ? (hero ? [canon.gen.hero, canon.gen.tall] : [canon.gen.tall, canon.gen.hero]) : [];
  const real = [...base, ...more].filter((s) => imageOk(s, privacy)) as string[];
  const chain = [...real, ...gen];
  const [i, setI] = useState(0);
  const out = i >= chain.length;
  // Real art ran out: look the name up on Steam, unless we have generated art for it (those games are not on Steam).
  // The search sends the game's name to Steam, so it runs only when the player allows it.
  const search = !!privacy?.asked && privacy.storeArt && privacy.artSearch;
  const realOut = i >= real.length;
  useEffect(() => {
    if (!realOut || found || canon.gen || !search) return;
    let live = true;
    steamLookup(name).then((a) => live && a && setFound(a));
    return () => { live = false; };
  }, [realOut, found, !!canon.gen, name, search]);
  if (!out) {
    return (
      <div className={`art ${className}`} style={style}>
        <img src={chain[i]} alt="" draggable={false} onError={() => setI(i + 1)} loading="lazy" />
      </div>
    );
  }
  const g = cid ? GAME[cid] : undefined;
  const hue = g?.hue ?? hash(name) % 360;
  return (
    <div className={`art art-gen ${className}`} style={{ ...style, ['--h' as string]: hue }}>
      <span>{g?.short ?? name}</span>
    </div>
  );
}

/** Two games, one image: host and guest art meet on a diagonal seam of light. With a cover (the build's own image,
 *  from the live catalog) or a clip (a frame of the build's video, used as a still) the build fills the frame and the
 *  two games shrink to a thin split strip at its foot; a cover or clip that fails to load falls back to the split art. */
export function MashupCover({ host, guest, cover, clip, className = '' }: {
  host: string; guest?: string; cover?: string | null; clip?: string | null; className?: string;
}) {
  const guestKnown = guest && GAME[guest];
  const [broken, setBroken] = useState(false);
  const [clipBroken, setClipBroken] = useState(false);
  const privacy = usePrivacy();
  const split = (
    <>
      <GameArt id={host} name={GAME[host]?.name ?? host} className="cover-a" hero />
      {guest && <GameArt id={guestKnown ? guest : null} name={GAME[guest]?.name ?? guest} className="cover-b" hero />}
      {guest && <i className="seam" />}
    </>
  );
  const still = cover && !broken && imageOk(cover, privacy)
    ? <img className="cover-img" src={cover} alt="" draggable={false} loading="lazy" onError={() => setBroken(true)} />
    : clip && !clipBroken && imageOk(clip, privacy)
      ? <video className="cover-img" src={`${clip}#t=6`} preload="metadata" muted playsInline disablePictureInPicture onError={() => setClipBroken(true)} />
      : null;
  if (still) {
    return (
      <div className={`cover cover-still ${className}`}>
        {still}
        <div className="cover-strip">{split}</div>
      </div>
    );
  }
  return <div className={`cover ${className}`}>{split}</div>;
}

/** A launchpad agent's token image, small and round next to its name; nothing when it fails to load. */
export function Avatar({ src, size = 16 }: { src?: string | null; size?: number }) {
  const [broken, setBroken] = useState(false);
  const privacy = usePrivacy();
  if (!src || broken || !imageOk(src, privacy)) return null;
  return <img className="avatar" src={src} alt="" width={size} height={size} draggable={false} loading="lazy" onError={() => setBroken(true)} />;
}

export function Icon({ name, size = 20 }: { name: string; size?: number }) {
  const p: Record<string, ReactNode> = {
    mix: (<><rect x="3" y="4" width="7" height="16" rx="1.5" /><rect x="14" y="4" width="7" height="16" rx="1.5" /><path d="M10.5 12h3" /></>),
    library: (<><path d="M4 5v14M8 5v14" /><rect x="12" y="5" width="4" height="14" rx="1" transform="rotate(-12 14 12)" /><path d="M19 5v14" /></>),
    build: (<><path d="M12 3l2.2 5.3L20 9l-4.4 3.8L17 18.5 12 15.6 7 18.5l1.4-5.7L4 9l5.8-.7z" /></>),
    queue: (<><path d="M12 4v11M7 10l5 5 5-5" /><path d="M5 20h14" /></>),
    search: (<><circle cx="11" cy="11" r="6" /><path d="M20 20l-4.5-4.5" /></>),
    play: <path d="M8 5.5v13l11-6.5z" fill="currentColor" stroke="none" />,
    check: <path d="M5 12.5l4.5 4.5L19 7.5" />,
    x: <path d="M6 6l12 12M18 6L6 18" />,
    min: <path d="M6 12h12" />,
    max: <rect x="6" y="6" width="12" height="12" rx="1" />,
    plus: <path d="M12 5v14M5 12h14" />,
    server: (<><rect x="4" y="4" width="16" height="7" rx="1.5" /><rect x="4" y="13" width="16" height="7" rx="1.5" /><path d="M8 7.5h.01M8 16.5h.01" /></>),
    download: (<><path d="M12 4v11M7 10l5 5 5-5" /><path d="M5 20h14" /></>),
    clock: (<><circle cx="12" cy="12" r="8" /><path d="M12 7.5V12l3 2" /></>),
    restore: (<><path d="M4 12a8 8 0 1 0 2.4-5.7" /><path d="M4 4v4h4" /></>),
    refresh: (<><path d="M20 12a8 8 0 1 1-2.4-5.7" /><path d="M20 4v4h-4" /></>),
    shield: <path d="M12 3l7 3v6c0 4.5-3 7.5-7 9-4-1.5-7-4.5-7-9V6z" />,
    live: <circle cx="12" cy="12" r="4" fill="currentColor" stroke="none" />,
    swap: (<><path d="M7 7h11l-3-3M17 17H6l3 3" /></>),
    people: (<><circle cx="9" cy="8.5" r="3" /><path d="M3.5 19c.6-3.2 2.8-5 5.5-5s4.9 1.8 5.5 5" /><circle cx="16.5" cy="9.5" r="2.4" /><path d="M15.5 14.2c2.6-.3 4.5 1.4 5 4.3" /></>),
    link: (<><path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1" /><path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1" /></>),
    copy: (<><rect x="8" y="8" width="12" height="12" rx="2" /><path d="M16 8V5a1 1 0 0 0-1-1H5a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h3" /></>),
    ext: (<><path d="M14 4h6v6M20 4l-9 9" /><path d="M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" /></>),
  };
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      {p[name]}
    </svg>
  );
}

export const STORE_LABEL: Record<string, string> = { steam: 'Steam', epic: 'Epic', ubisoft: 'Ubisoft', gog: 'GOG', minecraft: 'Minecraft', ea: 'EA' };

export function fmtCount(n: number) {
  return n >= 1000 ? `${(n / 1000).toFixed(n >= 10000 ? 0 : 1)}k` : String(n);
}
