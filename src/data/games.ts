// Canonical games: one id per game whatever the store, so Skyrim on Steam and on GOG match the same mods.
// Store ids here are what the scanner reports (`steam:<appid>`, `epic:<AppName>`, ...).

export type CanonGame = {
  id: string;
  name: string;
  short: string;
  steam?: string[];
  epic?: RegExp;
  gog?: string[];
  minecraft?: boolean;
  /** Where to get it when the player does not own it. */
  store?: string;
  /** Tile colors for games with no public art. */
  hue?: number;
  /** Steam art at a non-standard path (newer apps serve it under hashed CDN folders, old ones as portrait.png). */
  steamArt?: { tall?: string; hero?: string; wide?: string };
};

/** Games with fal-generated look-alike key art in public/art (no logos, no text, no real characters; library/QC.md). */
const GENERATED = new Set(['minecraft', 'fortnite', 'jetpackjoyride', 'diablo', 'diablo2', 'skate3', 'pokemon', 'wiisports', 'brainrot', 'footballgame', 'hytale']);

const steamArt = (id: string) => `https://cdn.cloudflare.steamstatic.com/steam/apps/${id}/library_600x900.jpg`;
const steamWide = (id: string) => `https://cdn.cloudflare.steamstatic.com/steam/apps/${id}/header.jpg`;
// 1920x620 key art without the logo: what mashup covers are made of.
const steamHero = (id: string) => `https://cdn.cloudflare.steamstatic.com/steam/apps/${id}/library_hero.jpg`;

export const GAMES: CanonGame[] = [
  { id: 'gta5', name: 'Grand Theft Auto V', short: 'GTA V', steam: ['271590', '3240220'], epic: /^9d2d0eb64d5c44529cece33fe2a46482$/i, store: 'https://store.steampowered.com/app/3240220' },
  { id: 'minecraft', name: 'Minecraft: Java Edition', short: 'Minecraft', minecraft: true, store: 'https://www.minecraft.net/store/minecraft-java-bedrock-edition-pc', hue: 120 },
  { id: 'skyrim', name: 'The Elder Scrolls V: Skyrim Special Edition', short: 'Skyrim', steam: ['489830'], gog: ['1711230643'], store: 'https://store.steampowered.com/app/489830' },
  { id: 'fallout4', name: 'Fallout 4', short: 'Fallout 4', steam: ['377160'], gog: ['1998527297'], store: 'https://store.steampowered.com/app/377160' },
  { id: 'falloutnv', name: 'Fallout: New Vegas', short: 'New Vegas', steam: ['22380'], store: 'https://store.steampowered.com/app/22380' },
  { id: 'eldenring', name: 'Elden Ring', short: 'Elden Ring', steam: ['1245620'], store: 'https://store.steampowered.com/app/1245620' },
  { id: 'doom', name: 'DOOM + DOOM II', short: 'Doom', steam: ['2280'], gog: ['1440164514'], store: 'https://store.steampowered.com/app/2280' },
  { id: 'tf2', name: 'Team Fortress 2', short: 'TF2', steam: ['440'], store: 'https://store.steampowered.com/app/440' },
  { id: 'gmod', name: "Garry's Mod", short: 'GMod', steam: ['4000'], store: 'https://store.steampowered.com/app/4000' },
  { id: 'portal2', name: 'Portal 2', short: 'Portal 2', steam: ['620'], store: 'https://store.steampowered.com/app/620' },
  { id: 'terraria', name: 'Terraria', short: 'Terraria', steam: ['105600'], store: 'https://store.steampowered.com/app/105600' },
  { id: 'lethal', name: 'Lethal Company', short: 'Lethal Co.', steam: ['1966720'], store: 'https://store.steampowered.com/app/1966720' },
  { id: 'repo', name: 'R.E.P.O.', short: 'R.E.P.O.', steam: ['3241660'], store: 'https://store.steampowered.com/app/3241660' },
  { id: 'balatro', name: 'Balatro', short: 'Balatro', steam: ['2379780'], store: 'https://store.steampowered.com/app/2379780' },
  { id: 'teardown', name: 'Teardown', short: 'Teardown', steam: ['1167630'], store: 'https://store.steampowered.com/app/1167630' },
  { id: 'beamng', name: 'BeamNG.drive', short: 'BeamNG', steam: ['284160'], store: 'https://store.steampowered.com/app/284160' },
  { id: 'cs16', name: 'Counter-Strike 1.6', short: 'CS 1.6', steam: ['10'], store: 'https://store.steampowered.com/app/10' },
  { id: 'cyberpunk', name: 'Cyberpunk 2077', short: 'Cyberpunk', steam: ['1091500'], gog: ['1423049311'], epic: /^Ginger$/i, store: 'https://store.steampowered.com/app/1091500' },
  { id: 'quake', name: 'Quake', short: 'Quake', steam: ['2310'], store: 'https://store.steampowered.com/app/2310' },
  { id: 'superhot', name: 'SUPERHOT', short: 'SUPERHOT', steam: ['322500'], store: 'https://store.steampowered.com/app/322500' },
  { id: 'skatebird', name: 'SkateBIRD', short: 'SkateBIRD', steam: ['971030'], store: 'https://store.steampowered.com/app/971030' },
  { id: 'mw2', name: 'Call of Duty: Modern Warfare 2 (2009)', short: 'MW2', steam: ['10180', '10190'], store: 'https://store.steampowered.com/app/10180' },
  { id: 'fivenightsatfreddys', name: "Five Nights at Freddy's", short: 'FNAF', steam: ['319510'], steamArt: { tall: 'https://cdn.cloudflare.steamstatic.com/steam/apps/319510/portrait.png' }, store: 'https://store.steampowered.com/app/319510' },
  { id: 'overwatch', name: 'Overwatch 2', short: 'Overwatch', steam: ['2357570'], store: 'https://store.steampowered.com/app/2357570' },
  { id: 'kingdomtwocrowns', name: 'Kingdom Two Crowns', short: 'Kingdom', steam: ['701160'], store: 'https://store.steampowered.com/app/701160' },
  { id: 'babaisyou', name: 'Baba Is You', short: 'Baba Is You', steam: ['736260'], store: 'https://store.steampowered.com/app/736260' },
  { id: 'ashorthike', name: 'A Short Hike', short: 'A Short Hike', steam: ['1055540'], store: 'https://store.steampowered.com/app/1055540' },
  { id: 'celeste', name: 'Celeste', short: 'Celeste', steam: ['504230'], store: 'https://store.steampowered.com/app/504230' },
  { id: 'mirrorsedge-catalyst', name: "Mirror's Edge Catalyst", short: "Mirror's Edge", steam: ['1233570'], store: 'https://store.steampowered.com/app/1233570' },
  // Not on Steam: generated key art (public/art), with their real names.
  { id: 'fortnite', name: 'Fortnite', short: 'Fortnite', epic: /^Fortnite$/i, hue: 265 },
  { id: 'jetpackjoyride', name: 'Jetpack Joyride', short: 'Jetpack Joyride', hue: 30 },
  { id: 'pokemon', name: 'Pokémon', short: 'Pokémon', hue: 50 },
  { id: 'wiisports', name: 'Wii Sports', short: 'Wii Sports', hue: 190 },
  { id: 'brainrot', name: 'Brainrot', short: 'Brainrot', hue: 320 },
  { id: 'footballgame', name: 'Football Game', short: 'Football', hue: 120 },
  { id: 'zomboid', name: 'Project Zomboid', short: 'Zomboid', steam: ['108600'], store: 'https://store.steampowered.com/app/108600' },
  { id: 'acvalhalla', name: "Assassin's Creed Valhalla", short: 'AC Valhalla', steam: ['2208920'], store: 'https://store.steampowered.com/app/2208920', hue: 30 },
  // Mashup library (community fusions, 2026-10-05).
  { id: 'ultrakill', name: 'ULTRAKILL', short: 'ULTRAKILL', steam: ['1229490'], store: 'https://store.steampowered.com/app/1229490' },
  { id: 'slimerancher', name: 'Slime Rancher', short: 'Slime Rancher', steam: ['433340'], store: 'https://store.steampowered.com/app/433340' },
  { id: 'outerwilds', name: 'Outer Wilds', short: 'Outer Wilds', steam: ['753640'], store: 'https://store.steampowered.com/app/753640' },
  { id: 'peak', name: 'PEAK', short: 'PEAK', steam: ['3527290'], steamArt: {
    tall: 'https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/3527290/480bd879ac737921bfa2529a6fea15961267ad21/library_600x900.jpg',
    hero: 'https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/3527290/d75184257596a3d2b402c58db0ef28844804e952/library_hero.jpg',
  }, store: 'https://store.steampowered.com/app/3527290' },
  { id: 'readyornot', name: 'Ready or Not', short: 'Ready or Not', steam: ['1144200'], store: 'https://store.steampowered.com/app/1144200' },
  { id: 'rdr2', name: 'Red Dead Redemption 2', short: 'RDR2', steam: ['1174180'], store: 'https://store.steampowered.com/app/1174180' },
  { id: 'bully', name: 'Bully: Scholarship Edition', short: 'Bully', steam: ['12200'], store: 'https://store.steampowered.com/app/12200' },
  { id: 'justcause2', name: 'Just Cause 2', short: 'Just Cause 2', steam: ['8190'], store: 'https://store.steampowered.com/app/8190' },
  { id: 'diablo', name: 'Diablo + Hellfire', short: 'Diablo', gog: ['1412601690'], store: 'https://www.gog.com/game/diablo' },
  { id: 'diablo2', name: 'Diablo II', short: 'Diablo II', hue: 0 },
  { id: 'skate3', name: 'Skate 3 (Xbox 360)', short: 'Skate 3', hue: 200 },
  { id: 'valheim', name: 'Valheim', short: 'Valheim', steam: ['892970'], store: 'https://store.steampowered.com/app/892970' },
  { id: 'saintsrow3', name: 'Saints Row: The Third Remastered', short: 'Saints Row 3', steam: ['978300'], store: 'https://store.steampowered.com/app/978300' },
  { id: 'hytale', name: 'Hytale', short: 'Hytale', store: 'https://hytale.com', hue: 30 },
  // GitHub sweep (2026-10-07) and bring-your-own-ROM mashups.
  { id: 'halomcc', name: 'Halo: The Master Chief Collection', short: 'Halo MCC', steam: ['976730'], store: 'https://store.steampowered.com/app/976730' },
  { id: 'halflife2', name: 'Half-Life 2', short: 'Half-Life 2', steam: ['220'], store: 'https://store.steampowered.com/app/220' },
  { id: 'dyinglight', name: 'Dying Light', short: 'Dying Light', steam: ['239140'], store: 'https://store.steampowered.com/app/239140' },
  { id: 'schedule1', name: 'Schedule I', short: 'Schedule I', steam: ['3164500'], store: 'https://store.steampowered.com/app/3164500' },
  // The full game and its free demo (the demo is what cu-hornet runs on).
  { id: 'casualtiesunknown', name: 'Casualties: Unknown', short: 'Casualties', steam: ['4576490', '4576510'], steamArt: {
    tall: 'https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/4576490/2876bd25f1c82af501ba6e1befa7c8cc5178ce07/library_capsule.jpg',
    hero: 'https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/4576490/ff891c3749212e837fd707579c8bc9dc0a58f221/library_hero.jpg',
    wide: 'https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/4576490/907888dce73f50c48410b5ab92971a0d95776b29/header.jpg',
  }, store: 'https://store.steampowered.com/app/4576490' },
  { id: 'silksong', name: 'Hollow Knight: Silksong', short: 'Silksong', steam: ['1030300'], store: 'https://store.steampowered.com/app/1030300' },
  { id: 'bo2', name: 'Call of Duty: Black Ops II', short: 'Black Ops II', steam: ['202970'], store: 'https://store.steampowered.com/app/202970' },
  { id: 'subnautica', name: 'Subnautica', short: 'Subnautica', steam: ['264710'], store: 'https://store.steampowered.com/app/264710' },
  { id: 'crashtrilogy', name: 'Crash Bandicoot N. Sane Trilogy', short: 'Crash Trilogy', steam: ['731490'], store: 'https://store.steampowered.com/app/731490' },
  // The player's own ROM (own_copies): never scanned, never sold here, so no store link.
  { id: 'sm64', name: 'Super Mario 64', short: 'Mario 64', hue: 0 },
];

export const GAME = Object.fromEntries(GAMES.map((g) => [g.id, g])) as Record<string, CanonGame>;

export type GameArtSet = { tall?: string; wide?: string; hero?: string; gen?: { tall: string; hero: string } };

/** Real store art first (Steam CDN); `gen` is the generated look-alike art, used only after every real source. */
export function art(id: string): GameArtSet {
  const g = GAME[id];
  const s = g?.steam?.[0];
  const out: GameArtSet = s ? { tall: g.steamArt?.tall ?? steamArt(s), wide: g.steamArt?.wide ?? steamWide(s), hero: g.steamArt?.hero ?? steamHero(s) } : {};
  if (GENERATED.has(id)) out.gen = { tall: `/art/${id}-tall.webp`, hero: `/art/${id}-hero.webp` };
  return out;
}

/** Maps a scanned game to its canonical id, or null when we have no mods for it yet. */
export function canonOf(g: { store: string; storeId: string }): string | null {
  for (const c of GAMES) {
    if (g.store === 'steam' && c.steam?.includes(g.storeId)) return c.id;
    if (g.store === 'epic' && c.epic?.test(g.storeId)) return c.id;
    if (g.store === 'gog' && c.gog?.includes(g.storeId)) return c.id;
    if (g.store === 'minecraft' && c.minecraft) return c.id;
  }
  return null;
}
