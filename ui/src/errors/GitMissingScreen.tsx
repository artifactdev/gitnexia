import { useState } from 'react';
import { api, errorMessage } from '../api/client';
import { useGitCheck } from './gitCheck';
import './errors.css';

interface InstallGuide {
  instructions: string;
  command?: string;
}

export function gitInstallGuide(platform: string): InstallGuide {
  const os = platform.toLowerCase();
  if (os.startsWith('win')) {
    return {
      instructions: 'Install Git for Windows. In the installer, choose “Git from the command line and also from 3rd-party software” so Git is added to PATH.',
    };
  }
  if (os.startsWith('mac')) {
    return {
      instructions: 'Install Git with the Xcode Command Line Tools or use the official Git download page.',
      command: 'xcode-select --install',
    };
  }
  if (os.startsWith('linux')) {
    return {
      instructions: 'Install Git with your distribution’s package manager. For Debian or Ubuntu:',
      command: 'sudo apt install git',
    };
  }
  return { instructions: 'Install Git for your operating system from the official download page.' };
}

export function GitMissingScreen() {
  const [busy, setBusy] = useState(false);
  const [openError, setOpenError] = useState<string | null>(null);
  const guide = gitInstallGuide(navigator.platform);

  async function openDownload() {
    setOpenError(null);
    try {
      await api.openUrl('https://git-scm.com/downloads');
    } catch (e) {
      setOpenError(errorMessage(e));
    }
  }

  return (
    <div role="alertdialog" aria-labelledby="git-missing-title" className="git-too-old">
      <h1 id="git-missing-title">GitNexia needs Git</h1>
      <p>Git was not found on this system. Install Git to use GitNexia.</p>
      <p>{guide.instructions}</p>
      {guide.command && <pre>{guide.command}</pre>}
      <p>After installing, restart GitNexia if needed, then check again.</p>
      <div className="git-problem-actions">
        <button type="button" className="git-too-old-retry" onClick={() => void openDownload()}>
          Open Git download page
        </button>
        <button
          type="button"
          className="git-too-old-retry"
          disabled={busy}
          onClick={() => {
            setBusy(true);
            void useGitCheck.getState().retry().finally(() => setBusy(false));
          }}
        >
          Check again
        </button>
      </div>
      {openError && <p role="alert">{openError}</p>}
    </div>
  );
}
