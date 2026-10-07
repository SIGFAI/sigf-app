// The live player (the Live tab): hls.js, bundled with the app (the CSP loads no outside script), the same rules as
// sigf.ai's players (web/front/src/live.ts keepLive, agent.ts keepPlayer). WebView2 has no native HLS, so it is always
// hls.js over Media Source: segments from https://sigf.ai (connect-src), the picture from a blob: URL (media-src).
import type HlsType from 'hls.js';

type Hls = HlsType;
type HlsCtor = typeof HlsType;

let lib: Promise<HlsCtor | null> | null = null;
/** hls.js (its light build: no subtitles, DRM or alternate audio, none of which these streams use), loaded on first play. */
export function loadHls(): Promise<HlsCtor | null> {
  lib ??= import('hls.js/light').then((m) => (m.default.isSupported() ? m.default : null), () => null);
  return lib;
}

/** A new hls.js on `url`, playing into `v`. No worker (the CSP has no worker-src), a few segments behind the edge. */
export function attach(H: HlsCtor, v: HTMLVideoElement, url: string): Hls {
  const h = new H({ enableWorker: false, liveSyncDurationCount: 3, backBufferLength: 30, manifestLoadingMaxRetry: 2 });
  h.loadSource(url);
  h.attachMedia(v);
  return h;
}

/** Lets go of a video for good: no more segment requests once the player closes. */
export function drop(v: HTMLVideoElement | null, h: Hls | null) {
  h?.destroy();
  if (v) {
    v.pause();
    v.removeAttribute('src');
    v.load();
  }
}

/**
 * Keeps a live video live, as on sigf.ai:
 *  - back on the window: jump to the live edge and play (a hidden window is throttled and falls behind);
 *  - frozen picture (the time stops moving while it should play, 8 s): jump to the live edge, then, if it is still
 *    frozen 8 s later, `restart()` (a new player on the same stream);
 *  - hls.js fatal errors: a network one reloads, a media one recovers, twice; then `restart()`.
 * Returns the function that stops the watch.
 */
export function keepLive(v: HTMLVideoElement, h: Hls, H: HlsCtor, restart: () => void): () => void {
  let last = -1, still = 0, nudged = false, errors = 0, done = false, started = false;
  const onPlaying = () => { started = true; };
  v.addEventListener('playing', onPlaying);
  const edge = () => {
    const live = h.liveSyncPosition;
    if (live != null && Number.isFinite(live) && live - v.currentTime > 3) v.currentTime = live;
    h.startLoad();
    v.play().catch(() => {});
  };
  const again = () => { if (!done) { stop(); restart(); } };
  const onVisible = () => { if (document.visibilityState === 'visible') { last = -1; still = 0; nudged = false; edge(); } };
  document.addEventListener('visibilitychange', onVisible);
  h.on(H.Events.ERROR, (_e, d) => {
    if (!d.fatal || done) return;
    if (++errors > 2) return again();
    if (d.type === H.ErrorTypes.MEDIA_ERROR) h.recoverMediaError(); else h.startLoad();
  });
  const tick = setInterval(() => {
    // Only a video that should be playing, in a visible window.
    if (done || !started || document.visibilityState !== 'visible' || v.ended) { last = -1; still = 0; return; }
    if (v.currentTime !== last) { last = v.currentTime; still = 0; nudged = false; errors = 0; return; }
    if ((still += 4) < 8) return;
    if (!nudged) { nudged = true; still = 0; edge(); } else again();
  }, 4000);
  const stop = () => {
    done = true;
    clearInterval(tick);
    document.removeEventListener('visibilitychange', onVisible);
    v.removeEventListener('playing', onPlaying);
  };
  return stop;
}
