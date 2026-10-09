// The Minecraft profile bar of the game page (docs/GAME-HUB.md section 4, "Minecraft"): which SIGF Prism instance
// ("SIGF <version> <Loader>") Modrinth and CurseForge mods install into, picked here and remembered on this PC, and
// Play for it. Without Prism Launcher: where to get it.
import { useEffect, useState } from 'react';
import { appPlatform, openUrl, prismDownload } from '../lib/api';
import { LOADER_NAME, mcProfileState, playMcProfile, profileName, setMcProfile, useMcProfile, type McLoader, type McProfileState } from '../lib/mcprofile';
import { useInstalledMods } from '../lib/mods';
import { Icon } from '../ui';
import { usePage } from './Workshop';
import { t } from '../i18n';

export function McProfileBar() {
  const page = usePage();
  const { profile, versions, key } = useMcProfile();
  const installed = useInstalledMods('minecraft');
  const [state, setState] = useState<McProfileState | null>(null);

  // Re-read after an install into it (the first one writes the instance).
  const count = installed?.length ?? 0;
  useEffect(() => {
    if (!profile) return;
    let live = true;
    mcProfileState(profile).then((s) => live && setState(s), () => live && setState(null));
    return () => {
      live = false;
    };
  }, [key, count]);

  if (!profile) return null;
  const list = versions?.versions ?? [{ id: profile.mc, loaders: [profile.loader] }];
  const here = list.find((v) => v.id === profile.mc);
  const loaders: McLoader[] = here?.loaders.length ? here.loaders : [profile.loader];
  const name = profileName(profile);

  const pickVersion = (mc: string) => {
    const v = list.find((x) => x.id === mc);
    const loader = v && !v.loaders.includes(profile.loader) ? (v.loaders.includes('fabric') ? 'fabric' : v.loaders[0]) : profile.loader;
    setMcProfile({ mc, loader });
  };
  const play = () => {
    page.ctx.flash(t('toast.launching', { name }));
    playMcProfile(profile).catch((e: { code?: string; message?: string }) => {
      if (e?.code === 'needs_launcher') void appPlatform().then((p) => openUrl(prismDownload(p)));
      page.ctx.flash(e?.code === 'needs_launcher' ? t('mc.needsPrism') : e?.code === 'no_profile' ? t('mc.noModsYet', { name }) : t('mods.err.failed', { message: e?.message ?? String(e) }));
    });
  };

  return (
    <div className="mc-profile">
      <div className="mc-profile-pick">
        <span className="mc-profile-label">{t('mc.profile')}</span>
        <select value={profile.mc} onChange={(e) => pickVersion(e.target.value)} aria-label={t('mc.version')}>
          {list.map((v) => <option key={v.id} value={v.id}>{v.id}</option>)}
        </select>
        <div className="seg" role="group" aria-label={t('mc.loader')}>
          {loaders.map((l) => (
            <button key={l} className={l === profile.loader ? 'on' : ''} onClick={() => setMcProfile({ mc: profile.mc, loader: l })}>{LOADER_NAME[l]}</button>
          ))}
        </div>
      </div>
      {state && !state.prism ? (
        <button className="act act-get" onClick={() => void appPlatform().then((p) => openUrl(prismDownload(p)))} title={t('mc.needsPrism')}>
          {t('joinSheet.getPrism')} <Icon name="ext" size={12} />
        </button>
      ) : (
        <button className="act act-play" onClick={play} disabled={!state?.exists || !state.launcher} title={state?.exists ? t('mc.playTitle', { name }) : t('mc.noModsYet', { name })}>
          <Icon name="play" size={14} /> {t('mc.play')}
        </button>
      )}
      <p className="mc-profile-hint">{state && !state.prism ? t('mc.needsPrism') : t('mc.hint', { name })}</p>
    </div>
  );
}
