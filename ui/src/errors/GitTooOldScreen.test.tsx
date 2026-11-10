import { render, screen } from '@testing-library/react';
import { expect, it, vi } from 'vitest';

const { appInfo, openUrl } = vi.hoisted(() => ({ appInfo: vi.fn(), openUrl: vi.fn() }));
vi.mock('../api/client', () => ({ api: { appInfo, openUrl } }));
import { GitTooOldScreen } from './GitTooOldScreen';
import { gitInstallGuide, GitMissingScreen } from './GitMissingScreen';
import { GitUnavailableScreen } from './GitUnavailableScreen';
import { useGitCheck } from './gitCheck';

const tooOld = { kind: 'GitTooOld', message: 'git 2.25.1 is too old; GitNexia needs 2.40 or newer', commandId: 1, stderr: null } as const;

it('explains the minimum and offers Retry', () => {
  render(<GitTooOldScreen error={tooOld} />);
  expect(screen.getByRole('alertdialog').textContent).toContain('GitNexia needs Git 2.40 or newer');
  expect(screen.getByRole('button', { name: 'Check again' })).toBeTruthy();
});

it('gives platform-specific install guidance when Git is missing', async () => {
  expect(gitInstallGuide('Win32').instructions).toContain('Git for Windows');
  expect(gitInstallGuide('MacIntel').command).toBe('xcode-select --install');
  expect(gitInstallGuide('Linux x86_64').command).toBe('sudo apt install git');
  render(<GitMissingScreen />);
  expect(screen.getByRole('alertdialog').textContent).toContain('Git was not found');
  expect(screen.getByRole('button', { name: 'Check again' })).toBeTruthy();
  openUrl.mockResolvedValueOnce(undefined);
  screen.getByRole('button', { name: 'Open Git download page' }).click();
  await vi.waitFor(() => expect(openUrl).toHaveBeenCalledWith('https://git-scm.com/downloads'));
});

it('shows a distinct error when Git cannot be started', () => {
  const unavailable = { kind: 'Io', message: 'failed to run git: Access is denied (os error 5)', commandId: 3, stderr: null } as const;
  render(<GitUnavailableScreen error={unavailable} />);
  expect(screen.getByRole('alertdialog').textContent).toContain('Git was found but could not be started');
  expect(screen.getByRole('alertdialog').textContent).toContain('Access is denied');
});

vi.spyOn(console, 'warn').mockImplementation(() => {});
it('classifies missing, too-old, and unstartable Git separately', async () => {
  appInfo.mockRejectedValueOnce(tooOld);
  await useGitCheck.getState().check();
  expect(useGitCheck.getState().problem?.kind).toBe('tooOld');
  appInfo.mockResolvedValueOnce({ appVersion: 'x', gitVersion: '2.47.1' });
  await useGitCheck.getState().check();
  expect(useGitCheck.getState().problem).toBeNull();
  appInfo.mockRejectedValueOnce({ kind: 'Io', message: 'failed to run git: No such file or directory (os error 2)', commandId: 2, stderr: null });
  await useGitCheck.getState().check();
  expect(useGitCheck.getState().problem?.kind).toBe('missing');
  useGitCheck.setState({ problem: null });
  appInfo.mockRejectedValueOnce({ kind: 'Io', message: 'failed to run git: Permission denied (os error 13)', commandId: 3, stderr: null });
  await useGitCheck.getState().check();
  expect(useGitCheck.getState().problem?.kind).toBe('unavailable');
  useGitCheck.setState({ problem: null });
  appInfo.mockRejectedValueOnce({ kind: 'Io', message: 'repository read failed', commandId: 4, stderr: null });
  await useGitCheck.getState().check();
  expect(useGitCheck.getState().problem).toBeNull();
  useGitCheck.setState({ problem: null });
  appInfo.mockRejectedValueOnce(new Error('transport'));
  await useGitCheck.getState().check();
  expect(useGitCheck.getState().problem).toBeNull();
});
