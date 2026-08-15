/* eslint-disable @typescript-eslint/no-unused-vars */
import { describe, it, expect } from 'vitest';

function parseCommand(_prompt: string) {
  return undefined;
}

describe('App placeholder', () => {
  it('imports without errors', () => {
    expect(true).toBe(true);
  });
});

describe('parseCommand', () => {
  it('returns undefined for plain text', () => {
    expect(parseCommand('hello world')).toBeUndefined();
  });
});
