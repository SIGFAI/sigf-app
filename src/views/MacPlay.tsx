// Play on Mac (app/docs/MAC-PLAY.md section 3): the game page's Mac controls, the guided setup sheet (engine, Windows
// folder, Windows Steam sign-in and install, Play) and the Games hub's "Play on Mac" section. Nothing downloads or
// starts without a click; status ticks by itself while the player installs the game in Windows Steam.
import { useEffect, useRef, useState } from 'react';
import type { Ctx } from '../App';
import { openUrl } from '../lib/api';
import { GAME, steamHero } from '../data/games';
import {
  clearMacError, currentStep, pickMacData, macAppid, macEngines, macReady, macSteps, openMacStore, playMac, removeMac, setupMac, stepDone, stepOfProgress, stopMac,
  useMacGames, useMacState, waitForInstall, type MacError, type MacGame, type MacRun, type MacState, type MacStepId, type MacTest, type RemoveWhat,
} from '../lib/macplay';
import { GameArt, Icon } from '../ui';
import { Section } from './shared';
import { bytes, useEscape } from './Workshop';
import { t, type Key } from '../i18n';

const STEAM_MAC = 'https://store.steampowered.com/about/';
const steamWide = (id: string) => `https://cdn.cloudflare.steamstatic.com/steam/apps/${id}/header.jpg`;

/** A short name for buttons ("Skyrim"), from the recipe's full one. */
const shortName = (g: MacGame) => GAME[g.game]?.short ?? g.name;

/** The engine names, joined: "Wine 11.18 + DXMT 0.72". */
function engineNames(g: MacGame): string {
  const { list } = macEngines(g);
  if (!list.length) return [g.wine?.engine, g.wine?.renderer, g.port?.engine, g.port?.via].filter(Boolean).join(' + ');
  return list.map((e) => [e.name ?? e.id, e.version].filter(Boolean).join(' ')).join(' + ');
}
const rendererName = (g: MacGame) => macEngines(g).list.find((e) => e.kind === 'renderer')?.name ?? g.wine?.renderer ?? '';
const portName = (g: MacGame) => macEngines(g).list.find((e) => e.kind === 'native')?.name ?? g.port?.engine ?? '';

/** How the game runs, in a few words. */
export function routeLabel(g: MacGame): string {
  if (g.route === 'wine') return t('mac.routeWine', { renderer: rendererName(g) });
  if (g.route === 'native-port') return t('mac.routePort', { engine: portName(g) });
  return t('mac.routeNative');
}

// ---------- Tags ----------

const RESULT: Record<MacTest['result'], Key> = { works: 'mac.testWorks', menu: 'mac.testMenu', broken: 'mac.testBroken' };

export function MacTags({ g }: { g: MacGame }) {
  const online = g.wine?.online;
  return (
    <div className="mac-tags">
      {g.tested.length === 0 ? (
        <span className="mac-tag untested" title={t('mac.untestedTitle')}><Icon name="clock" size={11} /> {t('mac.untested')}</span>
      ) : (
        g.tested.map((x, i) => (
          <span key={i} className={`mac-tag ${x.result}`} title={t('mac.testedBy', { by: x.by, on: x.on })}>
            <i />{t(RESULT[x.result])}<em>{x.on}</em>
          </span>
        ))
      )}
      {online === 'offline-only' && <span className="mac-tag warnish" title={t('mac.offlineTitle')}>{t('mac.offlineOnly')}</span>}
      {online === 'none' && <span className="mac-tag">{t('mac.singlePlayer')}</span>}
      {online === 'ok' && <span className="mac-tag">{t('mac.onlineOk')}</span>}
      {g.minMacos && <span className="mac-tag">{t('mac.minMacos', { version: g.minMacos })}</span>}
    </div>
  );
}

// ---------- Progress ----------

/** What a setup step is doing, in words, from the core's progress step name. */
function runText(run: MacRun | null, g: MacGame): string {
  const s = (run?.step ?? '').toLowerCase();
  if (/copy|data/.test(s)) return t('mac.doingCopy');
  if (/sha|verify|check/.test(s)) return t('mac.doingVerify');
  if (/wineboot|prefix/.test(s)) return t('mac.doingPrefix');
  if (/mono/.test(s)) return t('mac.doingMono');
  if (/vcrun|verb/.test(s)) return t('mac.doingVerbs');
  if (/dll|renderer/.test(s)) return t('mac.doingRenderer', { renderer: rendererName(g) || 'DXMT' });
  if (/steam/.test(s)) return t('mac.doingSteam');
  if (/download|engine|fetch/.test(s) || !s) return t('mac.doingDownload', { engine: engineNames(g) });
  return t('mac.doingOther');
}

function RunBar({ run, g }: { run: MacRun | null; g: MacGame }) {
  const done = run?.done ?? 0;
  const total = run?.total ?? 0;
  const pct = total > 0 ? Math.max(0, Math.min(100, (done / total) * 100)) : null;
  const detail = total > 1e5 ? t('mac.bytesOf', { done: bytes(done), total: bytes(total) }) : pct !== null ? `${Math.round(pct)}%` : '';
  return (
    <div className="mac-run">
      <div><b>{runText(run, g)}</b><small>{detail}</small></div>
      <em className={`bar ${pct === null ? 'indet' : ''}`} style={{ ['--p' as string]: `${pct ?? 0}%` }} />
    </div>
  );
}

// ---------- Errors ----------

function ErrorBox({ e, g, ctx, onDismiss }: { e: MacError; g: MacGame; ctx: Ctx; onDismiss: () => void }) {
  const text =
    e.kind === 'download' ? t('mac.errDownload', { message: e.message })
    : e.kind === 'sha' ? t('mac.errSha')
    : e.kind === 'steam_missing' ? t('mac.errSteamMissing')
    : e.kind === 'not_installed' ? t('mac.errNotInstalled', { game: shortName(g) })
    : e.kind === 'network' ? t('mac.errNetwork', { message: e.message })
    : e.kind === 'data_missing' ? t('mac.errDataMissing')
    : e.kind === 'macos_old' ? t('mac.errMacosOld', { version: g.minMacos })
    : e.kind === 'in_use' ? t('mac.errInUse', { message: e.message })
    : t('mac.errOther', { message: e.message });
  return (
    <div className="join-error mac-error" role="alert">
      <span><Icon name="shield" size={15} /> {text}</span>
      <div className="host-actions">
        {e.kind === 'steam_missing' && <button className="act act-get" onClick={() => void openUrl(STEAM_MAC)}>{t('mac.getSteamMac')} <Icon name="ext" size={12} /></button>}
        {(e.kind === 'sha' || e.kind === 'other') && <button className="act act-ghost" onClick={() => ctx.report(null)}><Icon name="bug" size={13} /> {t('mac.report')}</button>}
        <button className="act act-ghost" onClick={onDismiss}>{t('mac.dismiss')}</button>
      </div>
    </div>
  );
}

// ---------- The setup sheet ----------

const STEP_TITLE: Record<MacStepId, Key> = {
  engine: 'mac.stepEngine', windows: 'mac.stepWindows', steam: 'mac.stepSteam', data: 'mac.stepData', install: 'mac.stepInstall', play: 'mac.stepPlay',
};

function StepRow({ id, n, g, st, ctx, active, passed }: { id: MacStepId; n: number; g: MacGame; st: MacState; ctx: Ctx; active: boolean; passed: boolean }) {
  const s = st.status;
  const done = passed || stepDone(id, s, g);
  const run = st.run;
  const setupBusy = run?.kind === 'setup';
  const showRun = setupBusy && active;
  const busy = !!run;
  const name = shortName(g);
  const retry = !!st.error;
  const { bytes: engineBytes } = macEngines(g);
  const appid = macAppid(g);
  const port = g.route === 'native-port';
  const title = id === 'steam' ? t(port ? 'mac.stepSteamPort' : 'mac.stepSteam', { game: name }) : id === 'install' ? t('mac.stepInstall', { game: name }) : t(STEP_TITLE[id], { game: name });

  // One line under the title: what it is, with sizes.
  let sub = '';
  if (id === 'engine') sub = [engineNames(g), engineBytes ? bytes(engineBytes) : ''].filter(Boolean).join(' · ');
  else if (id === 'windows') sub = t('mac.subWindows', { game: name });
  else if (id === 'steam') sub = done ? t('mac.subSteamDone', { game: name }) : t('mac.subSteam');
  else if (id === 'data') sub = g.port?.data.from === 'steam-win' ? t('mac.subDataSteam', { engine: portName(g) }) : g.port?.data.from === 'gog-installer' ? t('mac.subDataGog', { engine: portName(g) }) : t('mac.subDataFolder');
  else if (id === 'install') sub = s?.store === 'missing' ? t('mac.subInstallNoSteam') : t('mac.subInstall');
  else if (id === 'play') sub = s?.running ? t('mac.subRunning') : routeLabel(g);

  const play = () => void playMac(g.game).then((ok) => ok && ctx.flash(t('mac.startingToast', { game: name })));
  const waiting = st.storeOpened && !done;

  let body: React.ReactNode = null;
  if (active && !done) {
    switch (id) {
      case 'engine':
        body = (
          <>
            <p>{g.route === 'wine' ? t('mac.engineBody', { renderer: rendererName(g) || 'DXMT' }) : port && g.port?.data.from === 'steam-win' ? t('mac.engineBodyPort', { engine: portName(g) }) : t('mac.engineBodyNative', { engine: portName(g) })}</p>
            {showRun ? <RunBar run={run} g={g} /> : (
              <div className="mac-actions">
                <button className="act act-get" disabled={busy} onClick={() => void setupMac(g)}>
                  <Icon name={retry ? 'restore' : 'download'} size={14} /> {retry ? t('common.tryAgain') : t('mac.setUp')}{engineBytes > 0 && !retry && <small>{bytes(engineBytes)}</small>}
                </button>
              </div>
            )}
          </>
        );
        break;
      case 'windows':
        body = (
          <>
            <p>{t('mac.windowsBody')}</p>
            {showRun ? <RunBar run={run} g={g} /> : (
              <div className="mac-actions">
                <button className="act act-get" disabled={busy} onClick={() => void setupMac(g)}>
                  <Icon name={retry ? 'restore' : 'laptop'} size={14} /> {retry ? t('common.tryAgain') : t('mac.prepareWindows')}
                </button>
              </div>
            )}
          </>
        );
        break;
      case 'steam':
        body = waiting || s?.store === 'running' ? (
          <div className="mac-wait">
            <span className="mac-ring" />
            <div>
              <b>{t('mac.waitInstall', { game: name })}</b>
              <small>{t('mac.waitInstallSub')}</small>
            </div>
            <button className="act act-ghost" disabled={busy} onClick={() => void openMacStore(g.game)}>{t('mac.openSteamAgain')}</button>
          </div>
        ) : (
          <>
            <ol className="together-steps mac-howto">
              <li>{t('mac.steamHow1')}</li>
              <li>{t('mac.steamHow2', { game: name })}</li>
              <li>{port ? t('mac.steamHow3Port') : t('mac.steamHow3')}</li>
            </ol>
            <div className="mac-actions">
              <button className={`act act-get ${run?.kind === 'store' ? 'act-busy ws-indet' : ''}`} disabled={busy} onClick={() => void openMacStore(g.game)}>
                <span>{run?.kind === 'store' ? t('mac.openingSteam') : t('mac.openSteam')}</span>
              </button>
              <span className="mac-private"><Icon name="shield" size={12} /> {t('mac.steamPrivate')}</span>
            </div>
          </>
        );
        break;
      case 'data':
        body = g.port?.data.from !== 'steam-win' ? (
          waiting ? (
            <div className="mac-wait">
              <span className="mac-ring" />
              <div>
                <b>{t('mac.waitData')}</b>
                <small>{t('mac.waitDataSub', { engine: portName(g) })}</small>
              </div>
              <button className="act act-ghost" disabled={busy} onClick={() => void playMac(g.game)}>{t('mac.openLauncherAgain')}</button>
            </div>
          ) : (
            <>
              <ol className="together-steps mac-howto">
                {g.port?.data.from === 'folder' ? (
                  <>
                    <li>{t('mac.folderHow1')}</li>
                    <li>{t('mac.folderHow2', { engine: portName(g) })}</li>
                  </>
                ) : (
                  <>
                    <li>{t('mac.importHow1', { engine: portName(g) })}</li>
                    <li>{t('mac.importHow2Gog')}</li>
                    <li>{t('mac.importHow3')}</li>
                  </>
                )}
              </ol>
              <div className="mac-actions">
                {g.port?.data.from === 'folder' ? (
                  <button className={`act act-get ${setupBusy ? 'act-busy ws-indet' : ''}`} disabled={busy} onClick={() => void pickMacData(g.game)}>
                    <span>{setupBusy ? t('mac.doingCopy') : t('mac.pickFolder')}</span>
                  </button>
                ) : (
                  <button className={`act act-get ${run?.kind === 'play' ? 'act-busy ws-indet' : ''}`} disabled={busy} onClick={() => void playMac(g.game).then((ok) => ok && waitForInstall(g.game))}>
                    <span>{t('mac.openLauncher', { engine: portName(g) })}</span>
                  </button>
                )}
                {g.port?.data.from === 'gog-installer' && GAME[g.game]?.store && (
                  <button className="act act-ghost" onClick={() => void openUrl(GAME[g.game]!.store!)}>{t('mac.getOnGog')} <Icon name="ext" size={12} /></button>
                )}
              </div>
            </>
          )
        ) : (
          <>
            <p>{t('mac.dataBody', { engine: portName(g), files: (g.port?.data.files ?? []).join(', ') })}</p>
            {showRun ? <RunBar run={run} g={g} /> : (
              <div className="mac-actions">
                <button className="act act-get" disabled={busy} onClick={() => void setupMac(g)}>
                  <Icon name={retry ? 'restore' : 'copy'} size={14} /> {retry ? t('common.tryAgain') : t('mac.copyData')}
                </button>
              </div>
            )}
          </>
        );
        break;
      case 'install':
        body = s?.store === 'missing' ? (
          <div className="mac-actions">
            <button className="act act-get" onClick={() => void openUrl(STEAM_MAC)}>{t('mac.getSteamMac')} <Icon name="ext" size={12} /></button>
          </div>
        ) : waiting ? (
          <div className="mac-wait">
            <span className="mac-ring" />
            <div>
              <b>{t('mac.waitInstall', { game: name })}</b>
              <small>{t('mac.waitInstallMacSub')}</small>
            </div>
            <button className="act act-ghost" onClick={() => void openUrl(`steam://install/${appid}`)}>{t('mac.openSteamAgain')}</button>
          </div>
        ) : (
          <div className="mac-actions">
            <button className="act act-get" onClick={() => { void openUrl(`steam://install/${appid}`); waitForInstall(g.game); }}>
              <Icon name="download" size={14} /> {t('mac.installInSteam')}
            </button>
          </div>
        );
        break;
      case 'play':
        body = s?.running ? (
          <div className="mac-actions">
            <span className="mac-live"><i /> {t('mac.running')}</span>
            <button className={`act ws-danger ${run?.kind === 'stop' ? 'act-busy ws-indet' : ''}`} disabled={busy} onClick={() => void stopMac(g.game)}>
              <Icon name="stop" size={11} /> <span>{run?.kind === 'stop' ? t('mac.stopping') : t('mac.stop')}</span>
            </button>
          </div>
        ) : (
          <div className="mac-actions">
            <button className={`act act-play act-big ${run?.kind === 'play' ? 'act-busy ws-indet' : ''}`} disabled={busy} onClick={play}>
              <Icon name="play" size={15} /> <span>{run?.kind === 'play' ? t('mac.starting') : t('mac.playGame', { game: name })}</span>
            </button>
          </div>
        );
        break;
    }
  }

  return (
    <li className={`mac-step ${done ? 'done' : active ? 'on' : ''} ${id === 'play' && s?.running ? 'live' : ''}`}>
      <i className="mac-num">{done ? <Icon name="check" size={12} /> : n}</i>
      <div className="mac-step-main">
        <div className="mac-step-head">
          <b>{title}</b>
          {done && <em>{t('mac.done')}</em>}
        </div>
        {sub && <small>{sub}</small>}
        {body && <div className="mac-step-body">{body}</div>}
      </div>
    </li>
  );
}

function RemovePanel({ g, st, onClose }: { g: MacGame; st: MacState; onClose: () => void }) {
  const disk = st.status?.disk ?? { engineBytes: 0, prefixBytes: 0 };
  const port = g.route === 'native-port';
  // A port that needs no game files has nothing but its engine to remove.
  const noData = port && g.port?.data.from === 'none';
  const [what, setWhat] = useState<RemoveWhat>(noData ? 'engine' : 'prefix');
  const removing = st.run?.kind === 'remove';
  const noWin = port && g.port?.data.from !== 'steam-win';
  const opts: { id: RemoveWhat; label: Key; sub: string; size: number }[] = [
    { id: 'prefix' as const, label: (noWin ? 'mac.rmData' : port ? 'mac.rmPrefixPort' : 'mac.rmPrefix') as Key, sub: t(noWin ? 'mac.rmDataSub' : port ? 'mac.rmPrefixPortSub' : 'mac.rmPrefixSub', { game: shortName(g) }), size: disk.prefixBytes },
    { id: 'engine' as const, label: 'mac.rmEngine' as Key, sub: t('mac.rmEngineSub', { engine: engineNames(g) }), size: disk.engineBytes },
    { id: 'all' as const, label: 'mac.rmAll' as Key, sub: t('mac.rmAllSub'), size: disk.prefixBytes + disk.engineBytes },
  ].filter((o) => !(noData && o.id === 'prefix'));
  const size = opts.find((o) => o.id === what)!.size;
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => box.current?.scrollIntoView({ behavior: 'smooth', block: 'nearest' }), []);
  return (
    <div className="mac-remove" ref={box}>
      <div className="mac-remove-opts" role="radiogroup">
        {opts.map((o) => (
          <button key={o.id} role="radio" aria-checked={what === o.id} className={what === o.id ? 'on' : ''} onClick={() => setWhat(o.id)} disabled={removing}>
            <b>{t(o.label)}<em>{bytes(o.size)}</em></b>
            <small>{o.sub}</small>
          </button>
        ))}
      </div>
      {what !== 'engine' && <p className="ws-note warn">{t('mac.rmSaves')}</p>}
      <div className="host-actions">
        <button className="act act-ghost" disabled={removing} onClick={onClose}>{t('common.cancel')}</button>
        <button className={`act ws-danger ${removing ? 'act-busy ws-indet' : ''}`} disabled={removing} onClick={() => void removeMac(g.game, what).then((ok) => ok && onClose())}>
          <span>{removing ? t('mac.removing') : t('mac.rmConfirm', { size: bytes(size) })}</span>
        </button>
      </div>
    </div>
  );
}

export function MacSheet({ ctx, g, onClose }: { ctx: Ctx; g: MacGame; onClose: () => void }) {
  const st = useMacState(g.game, 4000);
  const [removing, setRemoving] = useState(false);
  useEscape(onClose);
  const s = st.status;
  const steps = macSteps(g);
  const cur = currentStep(g, s);
  const appid = macAppid(g);
  const disk = s?.disk;
  const used = disk ? disk.engineBytes + disk.prefixBytes : 0;
  const unsupported = st.error?.kind === 'unsupported';
  const ready = macReady(g, s);
  // While setup runs, the step its progress belongs to is the active one (the status is read again when it ends).
  const active = st.run?.kind === 'setup' ? stepOfProgress(st.run.step, g) : cur;
  const at = Math.max(steps.indexOf(active), steps.indexOf(cur));

  return (
    <div className="scrim scrim-center" onClick={onClose}>
      <div className="mac-sheet" onClick={(e) => e.stopPropagation()} role="dialog" aria-label={t('mac.sheetTitle', { game: g.name })}>
        <div className="cover mac-cover">
          <GameArt hero id={g.game} name={g.name} wide={appid ? steamHero(appid) : undefined} />
        </div>
        <button className="detail-close" onClick={onClose} aria-label={t('common.close')}><Icon name="x" size={16} /></button>
        <div className="mac-body">
          <span className="eyebrow"><Icon name="laptop" size={13} /> {t('mac.eyebrow')} · {routeLabel(g)}</span>
          <h2>{g.name}</h2>
          <MacTags g={g} />

          {unsupported ? (
            <div className="ws-soon mac-unsupported">
              <span className="ws-soon-icon"><Icon name="laptop" size={22} /></span>
              <h3>{t('mac.unsupportedTitle')}</h3>
              <p>{t('mac.unsupportedBody')}</p>
            </div>
          ) : (
            <>
              {!s && !st.error ? (
                <div className="mac-skel"><i /><i /><i /></div>
              ) : (
                <>
                  <div className="mac-progress-head">
                    <span>{ready ? (s?.running ? t('mac.headRunning') : t('mac.headReady')) : t('mac.headStep', { n: at + 1, count: steps.length })}</span>
                    <i className="mac-pips">{steps.map((id, i) => <em key={id} className={stepDone(id, s, g) || (ready && id === 'play') ? 'done' : i === at ? 'on' : ''} />)}</i>
                  </div>
                  {st.error && <ErrorBox e={st.error} g={g} ctx={ctx} onDismiss={() => clearMacError(g.game)} />}
                  <ol className="mac-steps">
                    {steps.map((id, i) => <StepRow key={id} id={id} n={i + 1} g={g} st={st} ctx={ctx} active={i === at} passed={i < at} />)}
                  </ol>
                </>
              )}

              {g.notes.length > 0 && (
                <>
                  <h4 className="mac-h4">{t('mac.goodToKnow')}</h4>
                  <ul className="notes mac-notes">{g.notes.map((n, i) => <li key={i}>{n}</li>)}</ul>
                </>
              )}

              {g.route !== 'native' && s && (
                <div className="mac-foot">
                  {removing ? (
                    <RemovePanel g={g} st={st} onClose={() => setRemoving(false)} />
                  ) : (
                    <>
                      <span className="mac-disk">
                        <Icon name="disk" size={14} />
                        {used > 0 ? (
                          <>
                            <b>{t('mac.diskUsed', { size: bytes(used) })}</b>
                            {g.port?.data.from !== 'none' && <small>{t(g.route === 'native-port' && g.port?.data.from !== 'steam-win' ? 'mac.diskSplitData' : 'mac.diskSplit', { engine: bytes(disk!.engineBytes), prefix: bytes(disk!.prefixBytes) })}</small>}
                          </>
                        ) : (
                          <small>{t('mac.diskNone')}</small>
                        )}
                      </span>
                      {used > 0 && (
                        <button className="act act-ghost" disabled={!!st.run || s.running} title={s.running ? t('mac.stopFirst') : undefined} onClick={() => setRemoving(true)}>
                          <Icon name="trash" size={13} /> {t('mac.remove')}
                        </button>
                      )}
                    </>
                  )}
                </div>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
}

// ---------- The game page ----------

/** The Mac buttons in a game page's header: Play (ready), Running + Stop, or "Play on Mac" (opens the setup). */
export function MacHeroActions({ ctx, g, onOpen }: { ctx: Ctx; g: MacGame; onOpen: () => void }) {
  const st = useMacState(g.game);
  const s = st.status;
  const name = shortName(g);
  if (s?.running) {
    return (
      <>
        <span className="mac-live mac-live-hero"><i /> {t('mac.runningOnMac')}</span>
        <button className={`act ws-danger act-hero ${st.run?.kind === 'stop' ? 'act-busy ws-indet' : ''}`} disabled={!!st.run} onClick={() => void stopMac(g.game)}>
          <Icon name="stop" size={11} /> <span>{st.run?.kind === 'stop' ? t('mac.stopping') : t('mac.stop')}</span>
        </button>
      </>
    );
  }
  if (macReady(g, s)) {
    return (
      <>
        <button className="act act-ghost act-hero" onClick={onOpen} title={t('mac.setupTitle')}><Icon name="laptop" size={14} /> {t('mac.onMac')}</button>
        <button
          className={`act act-play act-hero ${st.run?.kind === 'play' ? 'act-busy ws-indet' : ''}`}
          disabled={!!st.run}
          onClick={() => void playMac(g.game).then((ok) => (ok ? ctx.flash(t('mac.startingToast', { game: name })) : onOpen()))}
        >
          <Icon name="play" size={14} /> <span>{st.run?.kind === 'play' ? t('mac.starting') : t('card.play')}</span>
        </button>
      </>
    );
  }
  const steps = macSteps(g);
  const at = steps.indexOf(currentStep(g, s));
  const setup = st.run?.kind === 'setup';
  const pct = setup && st.run?.total ? Math.round(((st.run.done ?? 0) / st.run.total) * 100) : null;
  return (
    <button className={`act act-get act-hero ${setup ? 'act-busy' : ''} ${setup && pct === null ? 'ws-indet' : ''}`} style={pct !== null ? { ['--p' as string]: `${pct}%` } : undefined} onClick={onOpen}>
      <Icon name="laptop" size={14} />
      <span>{setup ? t('mac.settingUp') : t('mac.playOnMac')}</span>
      {s && !setup && at > 0 && <small>{t('mac.stepShort', { n: at + 1, count: steps.length })}</small>}
      {pct !== null && <small>{pct}%</small>}
    </button>
  );
}

/** The strip under a game page's tabs while the game is not ready on this Mac: what Play on Mac does, and Set up. */
export function MacStrip({ g, onOpen }: { g: MacGame; onOpen: () => void }) {
  const st = useMacState(g.game);
  if (!st.status || macReady(g, st.status) || st.error?.kind === 'unsupported') return null;
  const body =
    g.route === 'wine' ? t('mac.stripWine', { game: shortName(g) })
    : g.route === 'native-port' ? t(g.port?.data.from === 'none' ? 'mac.stripPortNoData' : 'mac.stripPort', { game: shortName(g), engine: portName(g) })
    : t('mac.stripNative', { game: shortName(g) });
  return (
    <div className="trust mac-strip">
      <Icon name="laptop" size={16} />
      <span><b>{t('mac.stripTitle')}</b> {body}</span>
      <button className="act act-get" onClick={onOpen}>{t('mac.setUpShort')}</button>
    </div>
  );
}

// ---------- The hub ----------

function MacCard({ ctx, g, i }: { ctx: Ctx; g: MacGame; i: number }) {
  const st = useMacState(g.game);
  const s = st.status;
  const appid = macAppid(g);
  const ready = macReady(g, s);
  const steps = macSteps(g);
  const at = steps.indexOf(currentStep(g, s));
  const { bytes: eb } = macEngines(g);
  const state = !s ? '' : s.running ? t('mac.cardRunning') : ready ? t('mac.cardReady') : st.run?.kind === 'setup' ? t('mac.settingUp') : at === 0 ? (eb ? t('mac.cardStart', { size: bytes(eb) }) : t('mac.cardNotSet')) : t('mac.stepShort', { n: at + 1, count: steps.length });
  const used = s ? s.disk.engineBytes + s.disk.prefixBytes : 0;
  const test = g.tested[0];
  return (
    <button className={`ws-game mac-card ${ready ? 'ready' : ''} ${s?.running ? 'live' : ''}`} style={{ ['--i' as string]: i }} onClick={() => ctx.game(g.game, { mac: !ready })}>
      <div className="mac-card-art">
        <GameArt id={g.game} name={g.name} src={appid ? steamWide(appid) : undefined} wide={appid ? steamWide(appid) : undefined} />
        <span className="chip mac-route">{g.route === 'wine' ? t('mac.chipWine') : g.route === 'native-port' ? t('mac.chipPort') : t('mac.chipNative')}</span>
        <span className={`chip mac-test ${test ? test.result : 'untested'}`}>{test ? t(RESULT[test.result]) : t('mac.untested')}</span>
      </div>
      <span>
        <b>{g.name}</b>
        <small className="mac-card-state">
          {s?.running ? <i className="mac-dot live" /> : ready ? <i className="mac-dot ok" /> : null}
          {state}
          {used > 0 && !s?.running && <em>{bytes(used)}</em>}
        </small>
      </span>
    </button>
  );
}

/** The Games hub on macOS: Windows games with a Mac recipe, before any of them is set up (the scan cannot see them). */
export function MacHubSection({ ctx }: { ctx: Ctx }) {
  const list = useMacGames();
  if (!list || list.length === 0) return null;
  return (
    <Section title={t('mac.hubTitle')} sub={t('mac.hubSub')}>
      <div className="ws-games mac-games">
        {list.map((g, i) => <MacCard key={g.game} ctx={ctx} g={g} i={i} />)}
      </div>
    </Section>
  );
}

/** Opens the sheet once when the page is asked to (`nav.mac`, from a hub card). */
export function useOpenOnArrival(want: boolean | undefined, n: number | undefined, ready: boolean, open: () => void) {
  useEffect(() => {
    if (want && ready) open();
  }, [n, ready]);
}
