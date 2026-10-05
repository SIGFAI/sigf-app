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
};

const steamArt = (id: string) => `https://cdn.cloudflare.steamstatic.com/steam/apps/${id}/library_600x900.jpg`;
const steamWide = (id: string) => `https://cdn.cloudflare.steamstatic.com/steam/apps/${id}/header.jpg`;
// 1920x620 key art without the logo: what mashup covers are made of.
const steamHero = (id: string) => `https://cdn.cloudflare.steamstatic.com/steam/apps/${id}/library_hero.jpg`;

export const GAMES: CanonGame[] = [
  { id: 'gta5', name: 'Grand Theft Auto V', short: 'GTA V', steam: ['271590', '3240220'], epic: /^9d2d0eb64d5c44529cece33fe2a46482$/i, store: 'https://store.steampowered.com/app/3240220' },
  { id: 'minecraft', name: 'Minecraft: Java Edition', short: 'Minecraft', minecraft: true, store: 'https://www.minecraft.net/store/minecraft-java-bedrock-edition-pc', hue: 120 },
  { id: 'skyrim', name: 'The Elder Scrolls V: Skyrim Special Edition', short: 'Skyrim', steam: ['489830'], gog: ['1711230643'], store: 'https://store.steampowered.com/app/489830' },
  { id: 'fallout4', name: 'Fallout 4', short: 'Fallout 4', steam: ['377160'], gog: ['1998527297'], store: 'https://store.steampowered.com/app/377160' },
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
  { id: 'skatebird', name: 'SkateBIRD', short: 'SkateBIRD', steam: ['1112730'], store: 'https://store.steampowered.com/app/1112730' },
  { id: 'mw2', name: 'Call of Duty: Modern Warfare 2 (2009)', short: 'MW2', steam: ['10180'], store: 'https://store.steampowered.com/app/10180' },
  { id: 'zomboid', name: 'Project Zomboid', short: 'Zomboid', steam: ['108600'], store: 'https://store.steampowered.com/app/108600' },
  { id: 'acvalhalla', name: "Assassin's Creed Valhalla", short: 'AC Valhalla', steam: ['2208920'], store: 'https://store.steampowered.com/app/2208920', hue: 30 },
  // Mashup library (community fusions, 2026-10-05).
  { id: 'ultrakill', name: 'ULTRAKILL', short: 'ULTRAKILL', steam: ['1229490'], store: 'https://store.steampowered.com/app/1229490' },
  { id: 'slimerancher', name: 'Slime Rancher', short: 'Slime Rancher', steam: ['433340'], store: 'https://store.steampowered.com/app/433340' },
  { id: 'outerwilds', name: 'Outer Wilds', short: 'Outer Wilds', steam: ['753640'], store: 'https://store.steampowered.com/app/753640' },
  { id: 'peak', name: 'PEAK', short: 'PEAK', steam: ['3527290'], store: 'https://store.steampowered.com/app/3527290' },
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
];

export const GAME = Object.fromEntries(GAMES.map((g) => [g.id, g])) as Record<string, CanonGame>;

export function art(id: string): { tall?: string; wide?: string; hero?: string } {
  const s = GAME[id]?.steam?.[0];
  return s ? { tall: steamArt(s), wide: steamWide(s), hero: steamHero(s) } : {};
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
