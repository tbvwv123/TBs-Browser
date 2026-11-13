// src/lib/rules.ts를 호스트와 같은 표(rules-vectors.json)로 시험한다
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { patternMatches, chromeMatches, shortcutName, adblockSite } from "../src/lib/rules.ts";

const v = JSON.parse(readFileSync(new URL("./rules-vectors.json", import.meta.url), "utf8"));

test("patterns", () => {
  for (const c of v.patterns) assert.equal(patternMatches(c.pattern, c.text), c.match, JSON.stringify(c));
});

test("chromePatterns", () => {
  for (const c of v.chromePatterns) assert.equal(chromeMatches(c.pattern, c.url), c.match, JSON.stringify(c));
});

test("shortcuts", () => {
  for (const c of v.shortcuts) {
    assert.equal(shortcutName({ key: c.key, ctrlKey: c.ctrl, shiftKey: c.shift, altKey: c.alt }), c.ui, JSON.stringify(c));
  }
});

test("adblockSites", () => {
  for (const c of v.adblockSites) assert.equal(adblockSite(c.url), c.site, JSON.stringify(c));
});
