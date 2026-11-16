import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, expect, it, vi } from 'vitest';
import type { ForgeAccount } from '../api/gen/ForgeAccount';
import type { SshState } from '../api/gen/SshState';
import { SshManager } from './SshManager';

const mocks = vi.hoisted(() => ({ ssh: vi.fn(), openUrl: vi.fn(), pickSshKey: vi.fn(), copyText: vi.fn(), confirmAction: vi.fn() }));
vi.mock('../api/client', () => ({ api: { ssh: mocks.ssh, openUrl: mocks.openUrl, pickSshKey: mocks.pickSshKey }, errorMessage: (e: Error) => e.message }));
vi.mock('../api/transport', () => ({ copyText: mocks.copyText }));
vi.mock('../ui/ConfirmDialog', () => ({ confirmAction: mocks.confirmAction }));
const account: ForgeAccount = {
  id: 'code.example.com', host: 'code.example.com', kind: 'bitbucket', baseUrl: 'https://code.example.com/context',
  user: { id: 7, username: 'nova', name: 'Nova', avatarUrl: null, webUrl: '', email: null, emails: [] },
  storage: 'keyring', version: null, versionCheckedAt: 0, addedAt: 0,
};
const system: SshState = {
  key: null, keyPage: 'https://code.example.com/context/plugins/servlet/ssh/account/keys',
  configPath: '/fictional/home/.ssh/config', config: 'Host forge\n IdentityFile old-key\n',
  globalCommand: 'ssh -i old-key', environmentCommand: null,
};
const dedicated: SshState = { ...system, key: { privateKey: '/fictional/private-key', publicKey: 'ssh-ed25519 fictional-public-key', fingerprint: 'SHA256:fictional-fingerprint' } };
beforeEach(() => {
  vi.resetAllMocks();
  mocks.ssh.mockResolvedValue(system);
  mocks.copyText.mockResolvedValue(undefined);
  mocks.openUrl.mockResolvedValue(null);
});

it('keeps the generated key visible and offers manual registration when the PAT upload fails', async () => {
  mocks.ssh.mockResolvedValueOnce(system).mockResolvedValueOnce(dedicated).mockRejectedValueOnce(new Error('Token cannot manage SSH keys'));
  render(<SshManager profile="work" account={account} />);
  const passphrase = await screen.findByLabelText('Passphrase (optional; not saved)');
  fireEvent.change(passphrase, { target: { value: 'fictional passphrase' } });
  fireEvent.click(screen.getByRole('button', { name: 'Generate Ed25519 key' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Token cannot manage SSH keys');
  expect(screen.getByRole('alert')).toHaveTextContent('local key is kept');
  expect(screen.getByLabelText('Public key')).toHaveValue(dedicated.key?.publicKey);
  expect(passphrase).toHaveValue('');
  expect(mocks.ssh).toHaveBeenNthCalledWith(2, 'work', account.id, { kind: 'generate', passphrase: 'fictional passphrase' });
  expect(mocks.ssh).toHaveBeenNthCalledWith(3, 'work', account.id, { kind: 'upload' });
  fireEvent.click(screen.getByRole('button', { name: 'Copy public key' }));
  await waitFor(() => expect(mocks.copyText).toHaveBeenCalledWith(dedicated.key?.publicKey));
  fireEvent.click(screen.getByRole('button', { name: 'Open forge SSH settings' }));
  await waitFor(() => expect(mocks.openUrl).toHaveBeenCalledWith(system.keyPage));
  expect(mocks.ssh.mock.calls.some((c) => c[2].kind === 'useSystem')).toBe(false);
});

it('does not change global configuration without confirmation and supplies the expected previous contents', async () => {
  render(<SshManager profile="work" account={account} />);
  const config = await screen.findByLabelText(system.configPath);
  fireEvent.change(config, { target: { value: 'Host edited\n' } });
  mocks.confirmAction.mockResolvedValueOnce(false);
  fireEvent.click(screen.getByRole('button', { name: 'Save system SSH configuration' }));
  await waitFor(() => expect(mocks.confirmAction).toHaveBeenCalledTimes(1));
  expect(mocks.ssh).toHaveBeenCalledTimes(1);
  mocks.confirmAction.mockResolvedValueOnce(true);
  fireEvent.click(screen.getByRole('button', { name: 'Save system SSH configuration' }));
  await waitFor(() => expect(mocks.ssh).toHaveBeenCalledWith('work', account.id, { kind: 'saveSystemConfig', contents: 'Host edited\n', expected: system.config }));
});

it('selects an existing key without uploading when registration is disabled', async () => {
  render(<SshManager profile="personal" account={account} />);
  fireEvent.change(await screen.findByLabelText('Private key path'), { target: { value: '/fictional/existing-key' } });
  fireEvent.click(screen.getByRole('checkbox', { name: /Register public key/ }));
  fireEvent.click(screen.getByRole('button', { name: 'Use existing key' }));
  await waitFor(() => expect(mocks.ssh).toHaveBeenCalledTimes(2));
  expect(mocks.ssh).toHaveBeenLastCalledWith('personal', account.id, { kind: 'select', path: '/fictional/existing-key', passphrase: '' });
});

it('browses without importing or uploading until the user applies the selection', async () => {
  mocks.pickSshKey.mockResolvedValue('/fictional/chosen-key');
  render(<SshManager profile="personal" account={account} />);
  fireEvent.click(await screen.findByRole('button', { name: 'Browse private key' }));
  await waitFor(() => expect(screen.getByLabelText('Private key path')).toHaveValue('/fictional/chosen-key'));
  expect(mocks.ssh).toHaveBeenCalledTimes(1);
});

it('does not claim a local key exists when generation fails', async () => {
  mocks.ssh.mockResolvedValueOnce(system).mockRejectedValueOnce(new Error('ssh-keygen failed'));
  render(<SshManager profile="work" account={account} />);
  fireEvent.click(await screen.findByRole('button', { name: 'Generate Ed25519 key' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('ssh-keygen failed');
  expect(screen.getByRole('alert')).not.toHaveTextContent('local key is kept');
  expect(mocks.ssh).toHaveBeenCalledTimes(2);
});

it('preserves unsaved system configuration while registering a key', async () => {
  mocks.ssh.mockResolvedValue(dedicated);
  render(<SshManager profile="work" account={account} />);
  const config = await screen.findByLabelText(system.configPath);
  fireEvent.change(config, { target: { value: 'Host unsaved\n' } });
  fireEvent.click(screen.getByRole('button', { name: 'Register public key' }));
  await waitFor(() => expect(mocks.ssh).toHaveBeenCalledTimes(2));
  expect(config).toHaveValue('Host unsaved\n');
});
