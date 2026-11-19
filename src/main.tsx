import { useEffect, useState } from 'react';
import type { FormEvent } from 'react';
import { createRoot } from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import './styles.css';

type Role = 'primary' | 'secondary';
type Edge = 'left' | 'right' | 'top' | 'bottom';
type Phase = 'idle' | 'starting' | 'waiting' | 'connected' | 'reconnecting' | 'stopping' | 'error';
type Status = {
  phase: Phase;
  role: Role | null;
  code: string | null;
  address: string | null;
  peer: string | null;
  latency_ms: number | null;
  forwarding: boolean;
  message: string | null;
};

const labels: Record<Phase, string> = {
  idle: 'Disconnected',
  starting: 'Starting',
  waiting: 'Ready to pair',
  connected: 'Connected',
  reconnecting: 'Reconnecting',
  stopping: 'Stopping',
  error: 'Connection error',
};
const edges: Edge[] = ['left', 'right', 'top', 'bottom'];

function savedEdge(key: string, fallback: Edge): Edge {
  const stored = localStorage.getItem(key);
  return edges.find((edge) => edge === stored) ?? fallback;
}

function LayoutPreview({ from, to }: { from: Edge; to: Edge }) {
  const vertical = from === 'top' || from === 'bottom';
  const secondaryFirst = from === 'left' || from === 'top';
  const primary = <span className="screen" data-edge={from}>Primary</span>;
  const secondary = <span className="screen" data-edge={to}>Secondary</span>;
  return <div className="preview" data-vertical={vertical} aria-hidden="true">
    {secondaryFirst ? secondary : primary}
    <span className="arrows">{vertical ? '↕' : '⇄'}</span>
    {secondaryFirst ? primary : secondary}
  </div>;
}

function EdgeField({ label, value, change, disabled }: {
  label: string;
  value: Edge;
  change: (edge: Edge) => void;
  disabled: boolean;
}) {
  return <label className="field">
    {label}
    <select value={value} disabled={disabled} onChange={(event) => change(event.target.value as Edge)}>
      {edges.map((edge) => <option key={edge} value={edge}>{edge}</option>)}
    </select>
  </label>;
}

function App() {
  const [status, setStatus] = useState<Status | null>(null);
  const [role, setRole] = useState<Role>('primary');
  const [address, setAddress] = useState('');
  const [code, setCode] = useState('');
  const [from, setFrom] = useState<Edge>(() => savedEdge('primary-edge', 'left'));
  const [to, setTo] = useState<Edge>(() => savedEdge('secondary-edge', 'right'));
  const [discovered, setDiscovered] = useState<string[]>([]);
  const [scanMessage, setScanMessage] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [update, setUpdate] = useState<string | null>(null);
  const [updateMessage, setUpdateMessage] = useState('');
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [installing, setInstalling] = useState(false);
  const hasStatus = status !== null;
  const running = !!status?.role;
  const selected = status?.role ?? role;
  const disabled = running || busy || installing;

  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const next = await invoke<Status>('status');
        if (alive) setStatus(next);
      } catch {
        if (alive) setError('Open EdgeBridge in the Windows desktop app.');
      } finally {
        if (alive) timer = setTimeout(poll, 250);
      }
    }
    void poll();
    return () => { alive = false; clearTimeout(timer); };
  }, []);

  useEffect(() => {
    localStorage.setItem('primary-edge', from);
    localStorage.setItem('secondary-edge', to);
  }, [from, to]);

  useEffect(() => {
    if (running || selected !== 'primary' || !hasStatus) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    async function detect() {
      try {
        const results = await invoke<string[]>('discover');
        if (active) {
          setDiscovered(results);
          if (results.length === 1) setAddress((current) => current || results[0]);
        }
      } catch {
        // The manual search reports errors. Background discovery is optional.
      } finally {
        if (active) timer = setTimeout(detect, 5000);
      }
    }
    void detect();
    return () => { active = false; clearTimeout(timer); };
  }, [hasStatus, running, selected]);

  async function checkUpdates(manual: boolean) {
    setCheckingUpdate(true);
    setUpdateMessage('');
    try {
      const version = await invoke<string | null>('check_update');
      setUpdate(version);
      if (manual && !version) setUpdateMessage('EdgeBridge is up to date.');
    } catch {
      if (manual) setUpdateMessage('Could not check for updates. Try again later.');
    } finally {
      setCheckingUpdate(false);
    }
  }

  useEffect(() => { void checkUpdates(false); }, []);

  async function installUpdate() {
    if (!update) return;
    setInstalling(true);
    setUpdateMessage('');
    try {
      await invoke('install_update', { version: update });
    } catch (err) {
      setUpdateMessage(String(err));
      setInstalling(false);
    }
  }

  async function action(command: string, args?: Record<string, string>) {
    setError('');
    setBusy(true);
    try {
      await invoke(command, args);
      setStatus(await invoke<Status>('status'));
      if (command === 'connect' || command === 'stop') setCode('');
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function scan() {
    setScanning(true);
    setScanMessage('');
    try {
      const results = await invoke<string[]>('discover');
      setDiscovered(results);
      if (results.length === 1) setAddress(results[0]);
      if (!results.length) setScanMessage('No receiver found. Enter its IPv4 address below.');
    } catch {
      setScanMessage('Discovery failed. Enter the receiver’s IPv4 address below.');
    } finally {
      setScanning(false);
    }
  }

  function connect(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    void action('connect', { address, code, from, to });
  }

  return <main className="mx-auto max-w-2xl px-5 py-8 sm:px-9 sm:py-10">
    <header className="mb-7 flex items-center justify-between gap-3">
      <div className="flex items-center gap-3">
        <span className="brand" aria-hidden="true"><i /><i /></span>
        <h1 className="text-2xl font-semibold tracking-tight">EdgeBridge KVM</h1>
      </div>
      <div className="status" data-connected={status?.phase === 'connected'} role="status">
        <span aria-hidden="true" />{labels[status?.phase ?? 'idle']}
      </div>
    </header>

    <div className="roles mb-5" aria-label="This computer’s role">
      <button type="button" disabled={disabled} aria-pressed={selected === 'primary'} onClick={() => setRole('primary')}>Primary PC</button>
      <button type="button" disabled={disabled} aria-pressed={selected === 'secondary'} onClick={() => setRole('secondary')}>Secondary PC</button>
    </div>

    {selected === 'primary' ? <form className="panel" onSubmit={connect}>
      <h2>Connect to the secondary PC</h2>
      <p className="description">Pair this computer to share its mouse and keyboard.</p>
      <LayoutPreview from={from} to={to} />
      <div className="grid grid-cols-2 gap-3">
        <EdgeField label="Primary exit edge" value={from} change={setFrom} disabled={disabled} />
        <EdgeField label="Secondary entry and return edge" value={to} change={setTo} disabled={disabled} />
      </div>
      <label className="field mt-4" htmlFor="address">Secondary IPv4 address</label>
      <div className="flex flex-wrap gap-2">
        <input className="min-w-28 flex-1" id="address" value={address} required disabled={disabled} placeholder="192.168.1.42"
          onChange={(event) => setAddress(event.target.value)} autoComplete="off" spellCheck={false} />
        <button className="secondary" type="button" disabled={disabled || scanning} onClick={() => void scan()}>
          {scanning ? 'Scanning…' : 'Find on Wi-Fi'}
        </button>
      </div>
      {scanMessage && <p className="hint" role="status">{scanMessage}</p>}
      {discovered.length > 1 && !running && <ul className="flex flex-wrap gap-2 py-2" aria-label="Discovered receivers">
        {discovered.map((ip) => <li key={ip}><button className="secondary" type="button" onClick={() => setAddress(ip)}>{ip}</button></li>)}
      </ul>}
      {!running && <label className="field mt-4">
        Pairing code
        <input value={code} type="password" required disabled={busy} placeholder="EB1-…" spellCheck={false}
          autoComplete="off" onChange={(event) => setCode(event.target.value)} />
      </label>}
      <div className="actions">
        {running
          ? <button className="secondary" type="button" disabled={busy} onClick={() => void action('stop')}>Disconnect</button>
          : <button className="primary" disabled={busy || scanning} type="submit">Connect</button>}
        <span className="hint">Emergency return <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>F12</kbd></span>
      </div>
    </form> : <section className="panel" aria-labelledby="receiver-title">
      <h2 id="receiver-title">Allow control of this PC</h2>
      <p className="description">Start the receiver and share its pairing code with the primary PC.</p>
      {status?.address && <div className="fact"><span>Suggested IPv4 address</span><code>{status.address}</code></div>}
      {status?.code && <label className="field mt-4">
        Receiver pairing code
        <input readOnly spellCheck={false} value={status.code} onFocus={(event) => event.currentTarget.select()} />
        <span className="hint">Copy or share the code privately. Stopping the receiver revokes it.</span>
      </label>}
      <div className="actions">
        {running
          ? <button className="secondary" type="button" disabled={busy} onClick={() => void action('stop')}>Stop receiver</button>
          : <button className="primary" type="button" disabled={busy} onClick={() => void action('listen')}>Start receiver</button>}
      </div>
    </section>}

    {running && <section className="mt-4 grid gap-2 rounded-2xl border border-slate-200 bg-white px-4 py-3 text-sm sm:grid-cols-3"
      aria-label="Session status">
      <div><span className="metric">Control</span><strong>{status?.forwarding ? 'Secondary PC' : 'Local'}</strong></div>
      <div><span className="metric">Peer</span><strong>{status?.peer ?? '—'}</strong></div>
      <div><span className="metric">LAN round trip</span><strong>{status?.latency_ms == null ? '—' : status.latency_ms + ' ms'}</strong></div>
    </section>}
    <section className="mt-5 flex flex-wrap items-center gap-3" aria-label="Software updates">
      {update ? <>
        <button type="button" className="secondary" disabled={!hasStatus || disabled}
          onClick={() => void installUpdate()}>
          {installing ? 'Installing…' : `Install update ${update}`}
        </button>
        {running && <span className="hint">Stop the session to install.</span>}
      </> : <button type="button" className="secondary" disabled={checkingUpdate}
        onClick={() => void checkUpdates(true)}>
        {checkingUpdate ? 'Checking for updates…' : 'Check for updates'}
      </button>}
      {updateMessage && <span className="hint" role="status">{updateMessage}</span>}
    </section>
    {(error || status?.message) && <p className="notice" role={error ? 'alert' : 'status'}>{error || status?.message}</p>}
  </main>;
}

createRoot(document.getElementById('root')!).render(<App />);
