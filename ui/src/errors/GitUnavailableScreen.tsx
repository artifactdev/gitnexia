import { useState } from 'react';
import type { GbError } from '../api/gen/GbError';
import { useGitCheck } from './gitCheck';
import './errors.css';

export function GitUnavailableScreen({ error }: { error: GbError }) {
  const [busy, setBusy] = useState(false);
  return (
    <div role="alertdialog" aria-labelledby="git-unavailable-title" className="git-too-old">
      <h1 id="git-unavailable-title">Git was found but could not be started</h1>
      <p>{error.message}</p>
      <p>Check that Git is installed correctly and available on PATH, then try again.</p>
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
  );
}
