import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import App from '../App';
import { parseCommand } from '../tauri/commandParser';

describe('App', () => {
  it('renders loading state initially', () => {
    render(<App />);
    expect(screen.getByText('Loading...')).toBeDefined();
  });
});

describe('parseCommand', () => {
  it('parses create_agent', () => {
    const parsed = parseCommand('/create_agent greeter say hello');
    expect(parsed).toBeDefined();
    expect(parsed?.name).toBe('create_agent');
    expect(parsed?.args.name).toBe('greeter');
    expect(parsed?.args.prompt).toBe('say hello');
  });

  it('returns undefined for non-command', () => {
    expect(parseCommand('hello world')).toBeUndefined();
  });
});
