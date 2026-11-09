import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  createProfile: vi.fn(async () => ({ id: 'w', name: 'Work', color: '#000' })),
  switchProfile: vi.fn(async () => ({ settings: {}, profile: {}, profiles: [] })),
  saveProfile: vi.fn(async () => null),
  saveSettings: vi.fn(async () => null),
  avatar: vi.fn(async () => null),
  forgeAccounts: vi.fn(async (): Promise<unknown[]> => []),
}));
vi.mock('../api/client', () => ({
  api,
  errorMessage: (e: unknown) => String(e),
}));
vi.mock('../api/transport', () => ({ copyText: vi.fn(async () => {}), inTauri: () => false }));

const { useProfileDialog, ProfileDialog } = await import('./ProfileDialog');

describe('ProfileDialog: focus trap (fix round 1)', () => {
  afterEach(() => { useProfileDialog.setState({ mode: null }); });

  it('traps Tab/Shift+Tab inside the dialog and returns focus to the opener on close', () => {
    render(
      <>
        <button type="button">Opener</button>
        <ProfileDialog />
      </>,
    );
    const opener = screen.getByRole('button', { name: 'Opener' });
    opener.focus();
    act(() => useProfileDialog.getState().open('new'));

    const name = screen.getByLabelText('Profile name');
    expect(document.activeElement).toBe(name); // autoFocus wins over the trap's own fallback focus

    fireEvent.change(name, { target: { value: 'Work' } });
    const create = screen.getByRole('button', { name: 'Create' });
    const cancel = screen.getByRole('button', { name: 'Cancel' });

    create.focus();
    expect(fireEvent.keyDown(create, { key: 'Tab' })).toBe(false); // prevented: wraps
    expect(document.activeElement).toBe(name);

    expect(fireEvent.keyDown(name, { key: 'Tab', shiftKey: true })).toBe(false); // prevented: wraps back
    expect(document.activeElement).toBe(create);

    // An ordinary Tab in the middle of the dialog is left to the browser's own focus movement.
    cancel.focus();
    expect(fireEvent.keyDown(cancel, { key: 'Tab' })).toBe(true);

    expect(fireEvent.keyDown(create, { key: 'Escape' })).toBe(false);
    expect(useProfileDialog.getState().mode).toBeNull();
    expect(document.activeElement).toBe(opener);
  });
});

describe('ProfileDialog: an empty name', () => {
  afterEach(() => {
    useProfileDialog.setState({ mode: null });
    api.forgeAccounts.mockReset().mockResolvedValue([]);
  });

  it('saves the trimmed empty name, and the hint says what it does', async () => {
    const { useAppState } = await import('../app/state');
    const renameProfile = vi.fn();
    useAppState.setState({ renameProfile, profile: { ...useAppState.getState().profile, name: 'Work', color: '#4d88ff' } });
    render(<ProfileDialog />);
    act(() => useProfileDialog.getState().open('rename'));
    const name = screen.getByLabelText('Profile name');
    expect(name).toHaveAccessibleDescription(/empty to show just the color/);
    fireEvent.change(name, { target: { value: '   ' } });
    const save = screen.getByRole('button', { name: 'Save' });
    expect(save).toBeEnabled();
    fireEvent.click(save);
    await vi.waitFor(() => expect(renameProfile).toHaveBeenCalledExactlyOnceWith('', '#4d88ff', '', ''));
    await vi.waitFor(() => expect(useProfileDialog.getState().mode).toBeNull());
  });

  it('saves the commit name and email, which drive the profile avatar', async () => {
    const { useAppState } = await import('../app/state');
    const renameProfile = vi.fn();
    useAppState.setState({ renameProfile, profile: { ...useAppState.getState().profile, name: 'Work', color: '#4d88ff', gitName: 'Ada Lovelace', gitEmail: 'ada@example.com' } });
    render(<ProfileDialog />);
    act(() => useProfileDialog.getState().open('rename'));
    expect(screen.getByLabelText('Commit name')).toHaveValue('Ada Lovelace');
    expect(screen.getByLabelText('Email')).toHaveValue('ada@example.com');
    fireEvent.change(screen.getByLabelText('Commit name'), { target: { value: '  Grace Hopper  ' } });
    fireEvent.change(screen.getByLabelText('Email'), { target: { value: ' grace@example.com ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await vi.waitFor(() => expect(renameProfile).toHaveBeenCalledExactlyOnceWith('Work', '#4d88ff', 'Grace Hopper', 'grace@example.com'));
  });

  it('lists this profile’s accounts and opens the Accounts settings page', async () => {
    api.forgeAccounts.mockResolvedValue([{
      account: {
        id: 'gitlab.example.com', host: 'gitlab.example.com', kind: 'gitlab', baseUrl: null,
        user: { username: 'ada', name: 'Ada Lovelace' },
      },
      status: { kind: 'ok' },
    }]);
    const { useSettingsUi } = await import('../settings/schema');
    const { useAppState } = await import('../app/state');
    useSettingsUi.setState({ open: false, page: 'general' });
    useAppState.setState({ profile: { ...useAppState.getState().profile, name: 'Work' } });
    render(<ProfileDialog />);
    act(() => useProfileDialog.getState().open('rename'));

    expect(await screen.findByText('GitLab · @ada')).toBeTruthy();
    expect(screen.getByText('gitlab.example.com')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Edit accounts' }));

    expect(useSettingsUi.getState().open).toBe(true);
    expect(useSettingsUi.getState().page).toBe('accounts');
    expect(screen.queryByRole('dialog', { name: 'Edit profile' })).toBeNull();
  });

  it('shows when the profile has no connected accounts', async () => {
    api.forgeAccounts.mockResolvedValue([]);
    render(<ProfileDialog />);
    act(() => useProfileDialog.getState().open('rename'));
    expect(await screen.findByText('No integrations or accounts connected.')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Edit accounts' })).toBeTruthy();
  });
});
