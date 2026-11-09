// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from 'vitest';
import { migrateLegacyStoreKeys } from './legacyStoreKeys';

describe('GitVolt and GitNex localStorage keys', () => {
  beforeEach(() => localStorage.clear());

  it('copies old keys to GitNexia names once, without overwriting the new ones', () => {
    localStorage.setItem('gitvolt.theme', 'dark');
    localStorage.setItem('gitvolt.wipDraft.v2', '{"a":1}');
    localStorage.setItem('gitnex.theme', 'system');
    localStorage.setItem('gitnex.density.v1', 'compact');
    localStorage.setItem('gitnexia.theme', 'light');
    migrateLegacyStoreKeys();
    expect(localStorage.getItem('gitnexia.theme')).toBe('light');
    expect(localStorage.getItem('gitnexia.density.v1')).toBe('compact');
    expect(localStorage.getItem('gitnexia.wipDraft.v2')).toBe('{"a":1}');
    expect(localStorage.getItem('gitvolt.theme')).toBe('dark');
    expect(localStorage.getItem('gitnex.theme')).toBe('system');
  });

  it('prefers GitNex over GitVolt when both have the key', () => {
    localStorage.setItem('gitvolt.theme', 'dark');
    localStorage.setItem('gitnex.theme', 'system');
    migrateLegacyStoreKeys();
    expect(localStorage.getItem('gitnexia.theme')).toBe('system');
  });
});
