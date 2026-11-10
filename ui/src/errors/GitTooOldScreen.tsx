import { useState } from 'react';
import type { GbError } from '../api/gen/GbError';
import { useGitCheck } from './gitCheck';
import './errors.css';

/** The blocking screen (spec §5.5) for a Git version below the supported minimum. */
export function GitTooOldScreen({ error }: { error: GbError }) {
  const [busy, setBusy] = useState(false);
  return (
    <div role="alertdialog" aria-labelledby="git-too-old-title" className="git-too-old">
      <h1 id="git-too-old-title">GitNexia needs Git 2.40 or newer</h1>
      <p>{error.message}</p>
      <p>Upgrade Git, then check again:</p>
      <button type="button" className="git-too-old-retry" disabled={busy} onClick={() => { setBusy(true); void useGitCheck.getState().retry().finally(() => setBusy(false)); }}>Check again</button>
    </div>
  );
}
