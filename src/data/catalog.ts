// Sample data for `npm run dev` in a plain browser (no Rust core, no sigf.ai), so the screens can be designed offline.
// Every entry, name, author, count and rating below is invented. The app itself reads the live catalog
// (`/api/app/catalog`, docs/RECIPE-FORMAT.md section 5).

export type Kind = 'mod' | 'mashup' | 'passthrough';

export type Mashup = {
  id: string;
  /** The live catalog's current version (x.y.z); absent on seed entries. Lobbies pin it. */
  version?: string;
  name: string;
  /** The build's own name when the catalog shows a generic one as "Host × Guest" (live catalog only). */
  subtitle?: string | null;
  tagline: string;
  kind: Kind;
  host: string;
  guest?: string;
  /** Games the player needs, canonical ids. */
  needs: string[];
  by: { name: string; agent?: boolean; model?: string };
  license: string;
  sizeMb: number;
  installSeconds: number;
  strategy: string;
  steps: string[];
  plays: number;
  rating: number;
  updated: string;
  repo?: string;
  fresh?: boolean;
  /** Absent on seed entries: they can't really install yet. */
  recipeUrl?: string;
  /** The build's own image, an absolute URL (live catalog only). */
  cover?: string | null;
  /** The launchpad agent's token image, an absolute URL (live catalog only): shown small next to the maker. */
  avatar?: string | null;
  /** Clip of the build, played on hover. */
  clip?: string;
  /** "beta" for library fusions made by community modders (live catalog only). */
  status?: string | null;
  /** It can run on a free hosted server (the recipe's `server` block, live catalog only). */
  server?: { game: string; maxPlayers: number } | null;
  /** Outbound links (live catalog only): source repo, the modder's GitHub profile, bug tracker, releases. */
  links?: { repo: string; author: string | null; issues: string | null; releases: string | null };
};

export const CATALOG: Mashup[] = [
  {
    id: 'sigf/example-passthrough',
    name: 'Example Passthrough',
    tagline: 'Sample entry: two games run at once and share one world.',
    kind: 'passthrough',
    host: 'gta5',
    guest: 'minecraft',
    needs: ['gta5', 'minecraft'],
    by: { name: 'example', agent: true, model: 'Example Model' },
    license: 'MIT',
    sizeMb: 212,
    installSeconds: 70,
    strategy: 'game-dir-snapshot + mrpack',
    steps: ['Plugin for the host game', 'Fabric instance in Prism', 'Snapshot of the host files for Restore'],
    plays: 1200,
    rating: 4.5,
    updated: '2026-01-03',
    fresh: true,
  },
  {
    id: 'example/skyrim-minecraft',
    name: 'Example Fusion',
    tagline: 'Sample entry: a community fusion credited to its author.',
    kind: 'passthrough',
    host: 'skyrim',
    guest: 'minecraft',
    needs: ['skyrim', 'minecraft'],
    by: { name: 'example' },
    license: 'MIT',
    sizeMb: 340,
    installSeconds: 95,
    strategy: 'game-dir-snapshot + mrpack',
    steps: ['Script extender for your game build', 'Plugin into Data', 'Fabric instance in Prism'],
    plays: 900,
    rating: 4.4,
    updated: '2026-01-02',
  },
  {
    id: 'sigf/example-doom-mashup',
    name: 'Example Mashup',
    tagline: 'Sample entry: a mod loaded with a launch argument.',
    kind: 'mashup',
    host: 'doom',
    guest: 'Example Game',
    needs: ['doom'],
    by: { name: 'example', agent: true, model: 'Example Model' },
    license: 'MIT',
    sizeMb: 14,
    installSeconds: 4,
    strategy: 'args (-file mod.pk3)',
    steps: ['mod.pk3 into the app library', 'Launch argument only: nothing touches the game'],
    plays: 600,
    rating: 4.3,
    updated: '2026-01-02',
  },
  {
    id: 'sigf/example-tf2-mod',
    name: 'Example Mod',
    tagline: 'Sample entry: a Source game mod in an app-managed folder.',
    kind: 'mod',
    host: 'tf2',
    needs: ['tf2'],
    by: { name: 'example', agent: true, model: 'Example Model' },
    license: 'MIT',
    sizeMb: 22,
    installSeconds: 6,
    strategy: 'profile (custom folder)',
    steps: ['Scripts into tf/custom', 'Local listen server, bots only'],
    plays: 300,
    rating: 4.2,
    updated: '2026-01-01',
  },
  {
    id: 'sigf/example-minecraft-pack',
    name: 'Example Pack',
    tagline: 'Sample entry: a Minecraft mod as a Prism instance.',
    kind: 'mod',
    host: 'minecraft',
    needs: ['minecraft'],
    by: { name: 'example', agent: true, model: 'Example Model' },
    license: 'MIT',
    sizeMb: 58,
    installSeconds: 20,
    strategy: 'mrpack',
    steps: ['Fabric instance in Prism'],
    plays: 150,
    rating: 4.1,
    updated: '2026-01-01',
  },
];

export function pairKey(a: string, b: string) {
  return [a, b].sort().join('+');
}
