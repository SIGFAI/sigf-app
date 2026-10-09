// Browser games: third-party sites that run a game in the page (WASM ports, remakes, shareware). The app opens each in
// child webview inside the main window (Rust `web.rs`), with no access to the app's commands. Covers: public/art/web/<id>.webp;
// `art` is the name the Steam art lookup searches if a cover fails to load.

export type WebGame = { id: string; name: string; url: string; art?: string; note?: string };

export const WEB_GAMES: WebGame[] = [
  { id: 'bo1z', name: 'BO1 Zombies', url: 'https://vel.gg/bo1z', art: 'Call of Duty: Black Ops' },
  { id: 'moon', name: 'Moon', url: 'https://moon-zombies.pages.dev', art: 'Call of Duty: Black Ops', note: 'Zombies' },
  { id: 'kino', name: 'Kino der Toten', url: 'https://kino-der-toten.pages.dev', art: 'Call of Duty: Black Ops', note: 'Zombies' },
  { id: 'cheese-cube', name: 'BO3 Cheese Cube', url: 'https://cheese-cube.pages.dev', art: 'Call of Duty: Black Ops III', note: 'Zombies' },
  { id: 'bo2', name: 'Black Ops 2', url: 'https://vibeslops.luckeysystems.com', art: 'Call of Duty: Black Ops II' },
  { id: 'mw2', name: 'Modern Warfare 2', url: 'https://ovz-game-production.up.railway.app', art: 'Call of Duty: Modern Warfare 2' },
  { id: 'skate3', name: 'Skate 3', url: 'https://skate.aaddpp.lol', art: 'Skate 3' },
  { id: 'skate-rust', name: 'Skate Rust', url: 'https://global-terror.net' },
  { id: 'cs-surf', name: 'CS Surf', url: 'https://surfd.net', art: 'Counter-Strike: Source', note: 'Surf' },
  { id: 'halo-ce', name: 'Halo CE', url: 'https://mitchellhynes.com/halo', art: 'Halo: The Master Chief Collection' },
  { id: 'halo-ce-mobile', name: 'Halo CE mobile', url: 'https://hcemobile.com', art: 'Halo: The Master Chief Collection' },
  { id: 'pes6', name: 'PES 6', url: 'https://pes6.optijuegos.net', art: 'Pro Evolution Soccer' },
  { id: 'gta5', name: 'GTA 5', url: 'https://web.archive.org/web/20261006055917/https://playgta5.com/', art: 'Grand Theft Auto V', note: 'Archived page' },
  { id: 'gta-vc', name: 'GTA Vice City', url: 'https://joncodeofficial.github.io/gta-vice-city-wasm', art: 'Grand Theft Auto: Vice City' },
  { id: 'shar', name: 'The Simpsons: Hit & Run', url: 'https://shar-wasm.cjoseph.workers.dev/?skipmovie' },
  { id: 'q1', name: 'Quake', url: 'https://q1.pieter.com', art: 'Quake' },
  { id: 'q2', name: 'Quake II', url: 'https://q2.pieter.com', art: 'Quake II' },
  { id: 'q3', name: 'Quake III Arena', url: 'https://q3.pieter.com', art: 'Quake III Arena' },
  { id: 'rtcw', name: 'Return to Castle Wolfenstein', url: 'https://rtcw.pieter.com', art: 'Return to Castle Wolfenstein' },
  { id: 'ut', name: 'Unreal Tournament', url: 'https://ut.pieter.com', art: 'Unreal Tournament: Game of the Year Edition' },
  { id: 'hl', name: 'Half-Life', url: 'https://pixelsuft.github.io/hl/', art: 'Half-Life' },
  { id: 'xash', name: 'Half-Life / CS 1.6', url: 'https://x8bitrain.github.io/webXash/', art: 'Counter-Strike' },
  { id: 'diablo', name: 'Diablo', url: 'https://johnimril.github.io/diablo_web/', art: 'Diablo' },
  { id: 'hedgewars', name: 'Hedgewars', url: 'https://webwars.link', art: 'Hedgewars' },
];
