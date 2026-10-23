import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { featureUi } from './registry';
import type { FeatureUi } from './registry';

describe('feature registry', () => {
  it('covers exactly the features generated from ora-types', () => {
    const binding = readFileSync('src/native/bindings/Feature.ts', 'utf8');
    const union = /export type Feature = ([^;]+);/.exec(binding)?.[1] ?? '';
    const generated = [...union.matchAll(/"([A-Za-z]+)"/g)].map((m) => m[1]);
    expect(generated.length).toBeGreaterThan(0);
    expect(Object.keys(featureUi).sort()).toEqual([...generated].sort());
  });

  it('gives every deferred feature a visible reason', () => {
    // Every feature is wired since Phase 6e; later deferrals are still checked.
    for (const ui of Object.values(featureUi) as FeatureUi[]) {
      expect(ui.label).not.toBe('');
      if (ui.kind === 'deferred') expect(ui.reason).toMatch(/\S/);
    }
  });
});
