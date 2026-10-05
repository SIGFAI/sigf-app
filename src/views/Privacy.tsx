// Privacy (docs/PRIVACY.md): the first-start screen and the Privacy sheet. Same text, same choices. Keep the wording
// in step with docs/PRIVACY.md and src-tauri/windows/privacy.txt (the installer's first page).
import { useState, type ReactNode } from 'react';
import { openUrl } from '../lib/api';
import { savePrivacy, type Privacy } from '../lib/privacy';
import { Icon } from '../ui';

export const POLICY_URL = 'https://sigf.ai/privacy';

function Toggle({ on, onChange, disabled, title, children }: { on: boolean; onChange: (v: boolean) => void; disabled?: boolean; title: string; children: ReactNode }) {
  return (
    <label className={`pv-row ${disabled ? 'pv-off' : ''}`}>
      <input type="checkbox" checked={on && !disabled} disabled={disabled} onChange={(e) => onChange(e.target.checked)} />
      <i className="pv-switch" aria-hidden />
      <span>
        <b>{title}</b>
        <small>{children}</small>
      </span>
    </label>
  );
}

/**
 * The choices and what they mean. `first`: the first-start screen (nothing but the catalog request has gone out yet;
 * "Continue" saves and lets the app start its other requests). Otherwise the Privacy sheet, saved on "Save".
 */
export function PrivacyPanel({ initial, first = false, onDone, onClose }: { initial: Privacy; first?: boolean; onDone: (p: Privacy) => void; onClose?: () => void }) {
  const [p, setP] = useState<Privacy>(initial);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const set = (k: Partial<Privacy>) => setP((x) => ({ ...x, ...k }));

  const done = async () => {
    setBusy(true);
    setError(null);
    try {
      onDone(await savePrivacy({ ...p, artSearch: p.storeArt && p.artSearch, asked: true }));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
    setBusy(false);
  };

  return (
    <div className="scrim scrim-center pv-scrim" onClick={first ? undefined : onClose}>
      <div className="join-sheet pv-sheet" onClick={(e) => e.stopPropagation()} role="dialog" aria-labelledby="pv-title">
        {!first && onClose && <button className="detail-close" onClick={onClose} aria-label="Close"><Icon name="x" size={16} /></button>}
        <div className="pv-body">
          <span className="eyebrow"><Icon name="shield" size={12} /> Privacy</span>
          <h2 id="pv-title">{first ? 'Before SIGF goes online' : 'What SIGF sends, and to whom'}</h2>
          <p className="muted pv-lead">
            SIGF has no account, no ads, no telemetry, no analytics and no crash reports. It never reads your store logins or passwords.
            {first ? ' So far it has only asked sigf.ai for the mashup catalog. Choose what else it may do:' : ' You choose what it may do by itself:'}
          </p>

          <Toggle title="Game pictures" on={p.storeArt} onChange={(v) => set({ storeArt: v })}>
            Loads game, mashup and creator pictures from Steam, Epic Games and Modrinth image servers. They can tell which games are on your screen.
            Off: plain colored tiles (pictures already in Steam's cache on your PC still show).
          </Toggle>
          <Toggle title="Find missing pictures on Steam" on={p.artSearch} disabled={!p.storeArt} onChange={(v) => set({ artSearch: v })}>
            For a game with no picture (some Ubisoft, GOG and Epic games), sends the game's name to the Steam store search.
          </Toggle>
          <Toggle title="Lobbies for the games I own" on={p.lobbyGames} onChange={(v) => set({ lobbyGames: v })}>
            When the Lobbies tab or Play with friends is open, sends sigf.ai the games you own that SIGF has mashups for, so it lists only lobbies you can join.
            Off: SIGF gets every public lobby and picks yours on your PC.
          </Toggle>
          <div className="pv-row pv-row-seg">
            <span>
              <b>My local network address when I host</b>
              <small>A lobby you host carries a join address. Anyone with the invite link can see it.</small>
            </span>
            <div className="seg">
              <button type="button" className={p.lanAddress === 'ask' ? 'on' : ''} onClick={() => set({ lanAddress: 'ask' })}>Ask each time</button>
              <button type="button" className={p.lanAddress === 'auto' ? 'on' : ''} onClick={() => set({ lanAddress: 'auto' })}>Fill in for me</button>
            </div>
          </div>

          <details className="pv-always">
            <summary>Always, when you use a feature</summary>
            <ul>
              <li><b>sigf.ai</b>: the mashup catalog and the studio list when the app starts, a mashup's recipe when you install it, and the lobby you host or join (your display name, the lobby settings, the join address). sigf.ai keeps a salted hash of your IP address with a lobby to limit abuse.</li>
              <li><b>GitHub and Modrinth</b>: file downloads when you install a mashup or join a lobby.</li>
              <li><b>Your own game server</b>: a status check every 30 seconds while you host a Minecraft lobby.</li>
              <li><b>Never</b>: telemetry, analytics, crash reports, accounts, store logins or tokens.</li>
            </ul>
          </details>

          {error && <div className="join-error"><span>{error}</span></div>}
          <div className="host-actions pv-actions">
            <a className="pv-link" onClick={() => void openUrl(POLICY_URL)}>Full privacy policy <Icon name="ext" size={11} /></a>
            {!first && onClose && <button className="act act-ghost" onClick={onClose}>Cancel</button>}
            <button className="act act-get" onClick={() => void done()} disabled={busy} autoFocus>{first ? 'Continue' : 'Save'}</button>
          </div>
          <p className="muted pv-foot">You can change these any time: Privacy, at the bottom of the left bar.</p>
        </div>
      </div>
    </div>
  );
}
