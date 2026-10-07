import { useState } from 'react';
import type { Ctx } from '../App';
import { GAMES } from '../data/games';
import { GameArt, Icon } from '../ui';
import { t } from '../i18n';

export function Picker({ ctx, slot, onPick, onClose }: { ctx: Ctx; slot: 0 | 1; onPick: (id: string) => void; onClose: () => void }) {
  const [all, setAll] = useState(false);
  const list = GAMES.filter((g) => all || ctx.owned.has(g.id));

  return (
    <div className="scrim scrim-center" onClick={onClose}>
      <div className="picker" onClick={(e) => e.stopPropagation()}>
        <header>
          <div>
            <span className="eyebrow">{slot === 0 ? t('mix.host') : t('mix.guest')}</span>
            <h2>{slot === 0 ? t('picker.hostTitle') : t('picker.guestTitle')}</h2>
          </div>
          <div className="seg">
            <button className={!all ? 'on' : ''} onClick={() => setAll(false)}>{t('lib.title')}</button>
            <button className={all ? 'on' : ''} onClick={() => setAll(true)}>{t('picker.any')}</button>
          </div>
          <button className="detail-close" onClick={onClose} aria-label={t('common.close')}><Icon name="x" size={16} /></button>
        </header>
        {slot === 1 && <p className="hint">{t('picker.guestHint')}</p>}
        <div className="picker-grid">
          {list.map((g, i) => {
            const s = ctx.scan?.games.find((x) => x.canon === g.id);
            const taken = ctx.pair[slot === 0 ? 1 : 0] === g.id;
            return (
              <button key={g.id} className={`pick ${ctx.owned.has(g.id) ? '' : 'pick-dim'} ${taken ? 'pick-taken' : ''}`} onClick={() => onPick(g.id)} style={{ ['--i' as string]: i }}>
                <GameArt id={g.id} name={g.name} src={s?.art} wide={s?.artWide} local={s?.artLocal} wideLocal={s?.wideLocal} />
                <span>{g.short}</span>
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}
