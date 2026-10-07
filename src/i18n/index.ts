// UI language. English (en.ts) is the source and the default; every other dictionary is typed `Dict`, so a missing or
// extra key fails the build. Catalog content from sigf.ai (titles, taglines, how to play) and bug reports stay as sent.
import { Fragment, createElement, useSyncExternalStore, type ReactNode } from 'react';
import { en, type Dict, type Key, type Msg } from './en';
import { zhCN } from './zh-CN';
import { ja } from './ja';
import { ko } from './ko';
import { ptBR } from './pt-BR';

export type { Dict, Key } from './en';

export const LOCALES = ['en', 'zh-CN', 'ja', 'ko', 'pt-BR'] as const;
export type Locale = (typeof LOCALES)[number];
/** Each language under its own name, for the switch. */
export const LOCALE_NAMES: Record<Locale, string> = { en: 'English', 'zh-CN': '简体中文', ja: '日本語', ko: '한국어', 'pt-BR': 'Português (Brasil)' };

const DICTS: Record<Locale, Dict> = { en, 'zh-CN': zhCN, ja, ko, 'pt-BR': ptBR };

export const isLocale = (s: unknown): s is Locale => typeof s === 'string' && (LOCALES as readonly string[]).includes(s);

/** The system language mapped to a supported one: zh* -> zh-CN, ja* -> ja, ko* -> ko, pt* -> pt-BR, else en. */
export function systemLocale(langs: readonly string[] = typeof navigator === 'undefined' ? [] : navigator.languages?.length ? navigator.languages : [navigator.language]): Locale {
  for (const l of langs) {
    const s = (l || '').toLowerCase();
    if (s.startsWith('zh')) return 'zh-CN';
    if (s.startsWith('ja')) return 'ja';
    if (s.startsWith('ko')) return 'ko';
    if (s.startsWith('pt')) return 'pt-BR';
    if (s.startsWith('en')) return 'en';
  }
  return 'en';
}

let current: Locale = systemLocale();
const subs = new Set<() => void>();
const applyDoc = () => {
  if (typeof document !== 'undefined') document.documentElement.lang = current;
};
applyDoc();

/** The player's choice (saved with the privacy choices); null or unknown: the system language. */
export function setLocale(choice: string | null | undefined) {
  const next = isLocale(choice) ? choice : systemLocale();
  if (next === current) return;
  current = next;
  applyDoc();
  subs.forEach((f) => f());
}

export const getLocale = () => current;

export function useLocale(): Locale {
  return useSyncExternalStore(
    (f) => {
      subs.add(f);
      return () => subs.delete(f);
    },
    () => current,
  );
}

export type Vars = Record<string, string | number>;

// Intl formatters are costly to build: one per locale (and options), made on first use.
const formatters = new Map<string, unknown>();
function cached<T>(kind: string, loc: Locale, opts: object | undefined, make: () => T): T {
  const k = `${kind}|${loc}|${opts ? JSON.stringify(opts) : ''}`;
  let f = formatters.get(k) as T | undefined;
  if (!f) formatters.set(k, (f = make()));
  return f;
}
const plurals = (loc: Locale) => cached('plural', loc, undefined, () => new Intl.PluralRules(loc));
const dates = (loc: Locale, opts: Intl.DateTimeFormatOptions) => cached('date', loc, opts, () => new Intl.DateTimeFormat(loc, opts));

/** A number for the active language (or `loc`). */
export const num = (n: number, opts?: Intl.NumberFormatOptions, loc: Locale = current) =>
  cached('num', loc, opts, () => new Intl.NumberFormat(loc, opts)).format(n);
const fmtNum = (n: number, loc: Locale) => num(n, undefined, loc);

function pick(loc: Locale, key: Key, count: unknown): string {
  const m: Msg = DICTS[loc][key] ?? en[key];
  if (typeof m === 'string') return m;
  const n = typeof count === 'number' ? count : Number(count ?? 0);
  return m[plurals(loc).select(n)] ?? m.other;
}

/** `key` in `loc`, `{name}` filled from `vars` (numbers formatted for the locale; `count` picks the plural form). */
export function tIn(loc: Locale, key: Key, vars?: Vars): string {
  return pick(loc, key, vars?.count).replace(/\{(\w+)\}/g, (all, k: string) => {
    const v = vars?.[k];
    return v === undefined ? all : typeof v === 'number' ? fmtNum(v, loc) : v;
  });
}

/** `key` in the active language. */
export const t = (key: Key, vars?: Vars) => tIn(current, key, vars);

/** `key` with React nodes in its `{name}` slots (bold numbers, links, code). */
export function tx(key: Key, vars: Record<string, ReactNode>): ReactNode {
  const parts = pick(current, key, vars.count).split(/\{(\w+)\}/g);
  return createElement(
    Fragment,
    null,
    ...parts.map((p, i) => {
      if (i % 2 === 0) return p;
      const v = vars[p];
      return createElement(Fragment, { key: i }, typeof v === 'number' ? fmtNum(v, current) : v === undefined ? `{${p}}` : v);
    }),
  );
}

/** "A and B", "A, B, and C" in the active language (or `loc`). */
export const list = (items: string[], type: 'conjunction' | 'disjunction' = 'conjunction', loc: Locale = current) =>
  cached('list', loc, { type }, () => new Intl.ListFormat(loc, { type })).format(items);

const DAY: Intl.DateTimeFormatOptions = { month: 'short', day: 'numeric' };
const DATE: Intl.DateTimeFormatOptions = { year: 'numeric', month: 'short', day: 'numeric' };

/** A day as players read it here: "Oct 7", "10月7日". */
export function day(d: string | number | Date) {
  const x = new Date(d);
  return Number.isNaN(x.getTime()) ? 'Invalid Date' : dates(current, DAY).format(x); // format() throws where toLocaleDateString did not
}

/** A full date, or the text as it came when it is not one. */
export function date(s: string) {
  const d = new Date(s);
  return Number.isNaN(d.getTime()) ? s.slice(0, 10) : dates(current, DATE).format(d);
}
