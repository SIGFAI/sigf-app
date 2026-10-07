// UI language checks (npm run check-i18n): every dictionary has English's `{placeholders}` and an `other` plural form
// (tsc checks the keys); and no JSX in the views, App.tsx or ui.tsx shows raw English (text, a user-facing attribute, or
// a string literal rendered as a child). Product names and symbols are allowed.
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import ts from 'typescript';

const SRC = new URL('../src/', import.meta.url).pathname;
const LANGS = { 'zh-CN': 'zhCN', ja: 'ja', ko: 'ko', 'pt-BR': 'ptBR' };
const problems = [];

// --- dictionaries ---
const tmp = mkdtempSync(join(tmpdir(), 'sigf-i18n-'));
async function load(file, name) {
  const out = ts.transpileModule(readFileSync(join(SRC, 'i18n', `${file}.ts`), 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
  }).outputText.replace(/from '\.\/en'/g, "from './en.mjs'");
  const path = join(tmp, `${file}.mjs`);
  writeFileSync(path, out);
  return (await import(pathToFileURL(path).href))[name];
}
const forms = (m) => (typeof m === 'string' ? [m] : Object.values(m));
const holes = (m) => new Set(forms(m).flatMap((s) => [...s.matchAll(/\{(\w+)\}/g)].map((x) => x[1])));
try {
  const en = await load('en', 'en');
  const enHoles = new Map(Object.entries(en).map(([k, m]) => [k, holes(m)]));
  for (const [file, name] of Object.entries(LANGS)) {
    const d = await load(file, name);
    // Missing and extra keys fail tsc (every dictionary is typed `Dict`).
    for (const [k, want] of enHoles) {
      const m = d[k];
      if (typeof m !== 'string' && typeof m?.other !== 'string') problems.push(`${file}: ${k} has no "other" form`);
      // A translation may drop {count} when the language needs no number there; every other slot must stay.
      const got = holes(m);
      for (const h of want) if (h !== 'count' && !got.has(h)) problems.push(`${file}: ${k} lacks {${h}}`);
      for (const h of got) if (!want.has(h)) problems.push(`${file}: ${k} has unknown {${h}}`);
      if (file !== 'pt-BR' && forms(m).some((s) => /\b(the|and|your|with|this)\b/i.test(s.replace(/\{\w+\}/g, '')))) problems.push(`${file}: ${k} looks untranslated`);
    }
    console.log(`${file}: ${Object.keys(d).length} keys`);
  }
  console.log(`en: ${Object.keys(en).length} keys`);
} finally {
  rmSync(tmp, { recursive: true, force: true });
}

// --- raw English in JSX ---
const ALLOW = new Set(['SIGF', 'Steam', 'Epic', 'GOG', 'Ubisoft', 'EA', 'Prism', 'Minecraft', 'GitHub', 'Modrinth', 'MB', 'Ctrl', 'K', 'sigf', 'ai', 'v', 'k', 'M', 'ms']);
const ATTRS = new Set(['title', 'placeholder', 'aria-label', 'alt', 'label']);
// Whole strings shown as they are: product names, a Minecraft command.
const ALLOW_TEXT = new Set(['Prism Launcher', 'Modrinth App', '/publish true survival 25565']);
const english = (s) => !ALLOW_TEXT.has(s.trim()) && s.split(/[^A-Za-z]+/).some((w) => w.length > 1 && !ALLOW.has(w));
const files = [...readdirSync(join(SRC, 'views')).filter((f) => f.endsWith('.tsx')).map((f) => join('views', f)), 'App.tsx', 'ui.tsx'];
for (const f of files) {
  const text = readFileSync(join(SRC, f), 'utf8');
  const sf = ts.createSourceFile(f, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const at = (n) => `${f}:${sf.getLineAndCharacterOfPosition(n.getStart()).line + 1}`;
  const visit = (n, inChild) => {
    if (ts.isJsxText(n) && english(n.text)) problems.push(`${at(n)}: JSX text "${n.text.trim()}"`);
    if (ts.isJsxAttribute(n) && ATTRS.has(n.name.getText()) && n.initializer && ts.isStringLiteral(n.initializer) && english(n.initializer.text)) {
      problems.push(`${at(n)}: ${n.name.getText()}="${n.initializer.text}"`);
    }
    // A literal rendered as a child: `{cond ? 'Text' : x}`. Call arguments (`t('key')`) and object keys are not text.
    if (inChild && (ts.isStringLiteral(n) || ts.isNoSubstitutionTemplateLiteral(n) || ts.isTemplateExpression(n))) {
      const s = ts.isTemplateExpression(n) ? [n.head.text, ...n.templateSpans.map((x) => x.literal.text)].join(' ') : n.text;
      if (english(s)) problems.push(`${at(n)}: rendered string "${s}"`);
    }
    if (ts.isJsxAttribute(n) && ATTRS.has(n.name.getText()) && n.initializer && ts.isJsxExpression(n.initializer)) {
      ts.forEachChild(n.initializer, (c) => visit(c, true));
      return;
    }
    const child = ts.isJsxExpression(n) && n.parent && (ts.isJsxElement(n.parent) || ts.isJsxFragment(n.parent)) ? true
      : ts.isCallExpression(n) || ts.isJsxAttributes(n) || ts.isElementAccessExpression(n) || ts.isBinaryExpression(n) && n.operatorToken.kind !== ts.SyntaxKind.AmpersandAmpersandToken && n.operatorToken.kind !== ts.SyntaxKind.BarBarToken && n.operatorToken.kind !== ts.SyntaxKind.QuestionQuestionToken ? false
      : inChild;
    ts.forEachChild(n, (c) => visit(c, child));
  };
  visit(sf, false);
}

if (problems.length) {
  console.error(problems.join('\n'));
  console.error(`\n${problems.length} problem(s)`);
  process.exit(1);
}
console.log('i18n: dictionaries match, no raw English in the views');
