import { KeyRound } from 'lucide-react';
import { AccountsSection } from '../../forge/AccountsSection';
import { registerSettings, registerSettingsPage } from '../registry';
import { useSettingsUi } from '../schema';

function ManageAccounts() {
  return <button type="button" id="input-forgeAccounts" onClick={() => useSettingsUi.getState().setPage('accounts')}>Manage accounts</button>;
}

const offs = [
  registerSettingsPage({ id: 'accounts', label: 'Accounts', scope: 'profile', order: 30, icon: KeyRound, summary: 'Forge accounts for merge requests, pull requests, forks and avatars.', Body: () => <AccountsSection /> }),
  registerSettings([
    {
      id: 'forgeAccounts', label: 'Forge accounts', page: 'accounts', group: 'Accounts', labelFor: 'group',
      keywords: 'gitlab github forgejo gitea codeberg bitbucket azure devops organization repositories clone token personal access token pat account login keyring merge request pull request forks ssh key ed25519 passphrase agent',
      description: 'Forge accounts for repositories, merge requests, pull requests, forks and avatars. Azure DevOps Services supports repository access.',
      Control: ManageAccounts,
    },
  ]),
];
import.meta.hot?.dispose(() => offs.forEach((off) => off()));
